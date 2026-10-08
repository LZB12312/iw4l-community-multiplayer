use super::{apt_scene, runtime};
use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendState, Extent3d, PrimitiveTopology, RenderPipelineDescriptor, ShaderType,
        TextureDimension, TextureFormat,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin, MeshMaterial2d},
    ui_render::UiMaterialPlugin,
    window::PrimaryWindow,
};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug, ShaderType)]
pub(super) struct ColorTransform {
    pub multiply: Vec4,
    pub add: Vec4,
}
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct HudMaterial {
    #[uniform(0)]
    pub color: ColorTransform,
    #[texture(1)]
    #[sampler(2)]
    pub atlas: Handle<Image>,
}
impl Material2d for HudMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://ui/skate_ui/hud_render.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct HudComposite {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
}
impl UiMaterial for HudComposite {
    fn fragment_shader() -> ShaderRef {
        "embedded://ui/skate_ui/hud_composite.wgsl".into()
    }
    fn specialize(descriptor: &mut RenderPipelineDescriptor, _: UiMaterialKey<Self>) {
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
    }
}
struct Slot {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<HudMaterial>,
}
#[derive(Resource, Default)]
struct HudSetup {
    attempted: bool,
}
#[derive(Resource)]
struct Hud {
    target: Handle<Image>,
    composite: Handle<HudComposite>,
    rebind_after_resize: bool,
    runtime: runtime::Runtime,
    source: serde_json::Value,
    shapes: apt_scene::Shapes,
    textures: BTreeMap<String, Handle<Image>>,
    slots: Vec<Slot>,
    life: u32,
    impact: u32,
    accumulator: f32,
    speed: f32,
    previous_root: Option<(u32, Vec3)>,
    visible: bool,
    failed: bool,
}
pub(crate) struct HallOfMeatPlugin;
impl Plugin for HallOfMeatPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "hud_render.wgsl");
        embedded_asset!(app, "hud_composite.wgsl");
        app.add_plugins((
            Material2dPlugin::<HudMaterial>::default(),
            UiMaterialPlugin::<HudComposite>::default(),
        ))
        .init_resource::<HudSetup>()
        .add_systems(
            Update,
            (setup, resize_target, sync_output, advance, render)
                .chain()
                .after(frame::PresentedPublished)
                .in_set(frame::ClientSet::Present),
        );
    }
}
fn sync_output(
    cameras: Query<(Entity, &Camera), With<crate::UiCamera>>,
    mut composites: Query<&mut UiTargetCamera, With<MaterialNode<HudComposite>>>,
) {
    let selected = cameras
        .iter()
        .filter(|(_, camera)| camera.is_active)
        .max_by_key(|(entity, camera)| (camera.order, entity.to_bits()))
        .map(|(entity, _)| entity);
    if let Some(selected) = selected {
        for mut target in &mut composites {
            if target.0 != selected {
                target.0 = selected;
            }
        }
    }
}
fn setup(
    mut commands: Commands,
    mut setup: ResMut<HudSetup>,
    cameras: Query<(Entity, &Camera), With<crate::UiCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut composites: ResMut<Assets<HudComposite>>,
) {
    if setup.attempted {
        return;
    }
    let Some(output) = cameras
        .iter()
        .filter(|(_, camera)| camera.is_active)
        .max_by_key(|(entity, camera)| (camera.order, entity.to_bits()))
        .map(|(entity, _)| entity)
    else {
        return;
    };
    setup.attempted = true;
    let Some(assets) = std::env::var_os("IW4L_SKATE_ASSETS") else {
        return;
    };
    let root = PathBuf::from(assets).join("private/hom");
    if !root.join("runtime/homscoring.json").is_file() {
        return;
    }
    let result = (|| -> Result<Hud, String> {
        let source: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("runtime/homscoring.json"))
                .map_err(|e| format!("{}: {e}", root.display()))?,
        )
        .map_err(|e| e.to_string())?;
        let runtime = runtime::Runtime::load(&source)?;
        let shapes: apt_scene::Shapes =
            serde_json::from_value(source["shapes"].clone()).map_err(|e| e.to_string())?;
        let mut files = BTreeMap::new();
        for shape in shapes.values().flatten() {
            if let Some(texture) = &shape.texture {
                files.insert(texture.rgba.clone(), [texture.width, texture.height]);
            }
        }
        for font in runtime.bindings.movie.text_assets.fonts.values() {
            files.insert(font.texture.clone(), font.size);
        }
        let mut textures = BTreeMap::new();
        textures.insert(
            String::new(),
            images.add(Image::new(
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                vec![255; 4],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            )),
        );
        for (path, size) in files {
            let bytes = std::fs::read(root.join(&path)).map_err(|e| format!("HUD {path}: {e}"))?;
            if bytes.len() != size[0] as usize * size[1] as usize * 4 {
                return Err(format!("Invalid HUD texture size {path}"));
            }
            let image = Image::new(
                Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                bytes,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            textures.insert(path, images.add(image));
        }
        Ok(Hud {
            target: Handle::default(),
            composite: Handle::default(),
            rebind_after_resize: false,
            runtime,
            source,
            shapes,
            textures,
            slots: Vec::new(),
            life: 0,
            impact: 0,
            accumulator: 0.,
            speed: 0.,
            previous_root: None,
            visible: false,
            failed: false,
        })
    })();
    match result {
        Ok(mut hud) => {
            let target = images.add(Image::new_target_texture(
                window.physical_width().max(1),
                window.physical_height().max(1),
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            hud.target = target.clone();
            commands.spawn((
                Camera2d,
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: 1280.,
                        height: 720.,
                    },
                    ..OrthographicProjection::default_2d()
                }),
                Camera {
                    order: -100,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(target.clone().into()),
                RenderLayers::layer(31),
                Msaa::Off,
            ));
            hud.composite = composites.add(HudComposite { image: target });
            commands.spawn((
                MaterialNode(hud.composite.clone()),
                UiTargetCamera(output),
                GlobalZIndex(1),
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
            ));
            commands.insert_resource(hud);
            diag::info!(
                World,
                "Skate Hall of Meat HUD loaded from {}",
                root.display()
            );
        }
        Err(error) => diag::warn!(
            World,
            "Skate Hall of Meat HUD could not load from {}: {error}. Re-run Skate data preparation",
            root.display()
        ),
    }
}
fn resize_target(
    hud: Option<ResMut<Hud>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut composites: ResMut<Assets<HudComposite>>,
) {
    let Some(mut hud) = hud else {
        return;
    };
    if hud.rebind_after_resize {
        if let Some(mut material) = composites.get_mut(&hud.composite) {
            material.image = hud.target.clone();
        }
    }
    let size = Extent3d {
        width: window.physical_width().max(1),
        height: window.physical_height().max(1),
        depth_or_array_layers: 1,
    };
    hud.rebind_after_resize = resize_image(
        &mut images,
        &mut composites,
        &hud.target,
        &hud.composite,
        size,
    );
}
fn resize_image(
    images: &mut Assets<Image>,
    composites: &mut Assets<HudComposite>,
    target: &Handle<Image>,
    composite: &Handle<HudComposite>,
    size: Extent3d,
) -> bool {
    if images
        .get(target)
        .is_some_and(|image| image.texture_descriptor.size != size)
    {
        if let Some(mut image) = images.get_mut(target) {
            image.resize(size);
        }
        if let Some(mut material) = composites.get_mut(composite) {
            material.image = target.clone();
        }
        return true;
    }
    false
}
fn advance(
    hud: Option<ResMut<Hud>>,
    presented: Res<net::PresentedSnapshot>,
    local: Res<net::LocalPresentClient>,
    screen: Res<frame::AppScreen>,
    time: Res<Time>,
) {
    let Some(mut hud) = hud else {
        return;
    };
    if hud.failed {
        return;
    }
    let meta = presented
        .snapshot()
        .and_then(|s| s.meta.for_client(local.0).map(|m| (s.tick.0, m)));
    let mut input = runtime::Input::default();
    let mut visible = false;
    let mut new_impact = false;
    if *screen == frame::AppScreen::InGame
        && let Some((tick, meta)) = meta
    {
        if hud.life != meta.life_sequence.0 {
            match runtime::Runtime::load(&hud.source) {
                Ok(runtime) => {
                    hud.runtime = runtime;
                    hud.life = meta.life_sequence.0;
                    hud.impact = 0;
                    hud.speed = 0.;
                    hud.previous_root = None;
                }
                Err(error) => {
                    diag::warn!(World, "Hall of Meat reset: {error}");
                    hud.failed = true;
                    return;
                }
            }
        }
        if let Some(pose) = &meta.skate {
            let root = Vec3::new(pose.root[12], pose.root[13], pose.root[14]);
            if let Some((previous_tick, previous)) = hud.previous_root {
                let elapsed = tick
                    .wrapping_sub(previous_tick)
                    .saturating_mul(sim::MATCH_TICK_MS) as f32
                    * 0.001;
                if elapsed > 0. && elapsed < 1. && hud.impact == meta.skate_damage.impact.sequence {
                    hud.speed = ((root - previous).length() * 0.0254 / elapsed).min(100.);
                }
            }
            hud.previous_root = Some((tick, root));
        }
        let damage = meta.skate_damage;
        new_impact = damage.impact.sequence != hud.impact;
        hud.impact = damage.impact.sequence;
        let age = tick
            .wrapping_sub(damage.impact.tick)
            .saturating_mul(sim::MATCH_TICK_MS);
        visible = damage.display_active(tick);
        input = runtime::Input {
            score: damage.score,
            speed: hud.speed,
            duration: (age as f32 * 0.001).min(4.),
        };
    }
    hud.accumulator += time.delta_secs().min(0.1) * 60.;
    let frames = hud.accumulator.floor() as usize;
    hud.accumulator -= frames as f32;
    if let Err(error) = hud.runtime.update(input, visible, new_impact, frames) {
        diag::warn!(World, "Hall of Meat stopped: {error}");
        hud.failed = true;
    }
    hud.visible = *screen == frame::AppScreen::InGame;
}
fn render(
    mut commands: Commands,
    hud: Option<ResMut<Hud>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HudMaterial>>,
) {
    let Some(mut hud) = hud else {
        return;
    };
    let draws = if hud.failed || !hud.visible {
        Vec::new()
    } else {
        match apt_scene::draw(&hud.runtime.bindings.movie, &hud.runtime.vm, &hud.shapes) {
            Ok(draws) => draws,
            Err(error) => {
                diag::warn!(World, "Hall of Meat HUD geometry: {error}");
                hud.failed = true;
                Vec::new()
            }
        }
    };
    for (index, draw) in draws.iter().enumerate() {
        let Some(texture) = hud.textures.get(&draw.texture).cloned() else {
            continue;
        };
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            draw.vertices
                .iter()
                .map(|v| [v.position[0] - 640., 360. - v.position[1], 0.])
                .collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            draw.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vec![[0., 0., 1.]; draw.vertices.len()],
        );
        let material = HudMaterial {
            color: ColorTransform {
                multiply: draw.multiply.into(),
                add: draw.add.into(),
            },
            atlas: texture,
        };
        if index == hud.slots.len() {
            let mesh = meshes.add(mesh);
            let material = materials.add(material);
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., index as f32 * 0.01),
                    RenderLayers::layer(31),
                ))
                .id();
            hud.slots.push(Slot {
                entity,
                mesh,
                material,
            });
        } else {
            let slot = &hud.slots[index];
            if let Some(mut old) = meshes.get_mut(&slot.mesh) {
                *old = mesh;
            }
            if let Some(mut old) = materials.get_mut(&slot.material) {
                *old = material;
            }
            commands.entity(slot.entity).insert(Visibility::Visible);
        }
    }
    for slot in &hud.slots[draws.len()..] {
        commands.entity(slot.entity).insert(Visibility::Hidden);
    }
}
