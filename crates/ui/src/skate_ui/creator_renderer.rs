use super::{
    apt_scene,
    creator_preview::{PreviewPlugin, PreviewState},
    creator_runtime::{Key, Runtime},
    renderer::{ColorTransform, HudComposite, HudMaterial},
};
use bevy::{
    asset::RenderAssetUsages,
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
    sprite_render::MeshMaterial2d,
    window::PrimaryWindow,
};
use frame::{
    SkateCreatorState, UiCharacterEdit, UiCharacterEditResult, UiCharacterInput, UiMenuKey,
    UiMenuRequest,
};
use std::{collections::BTreeMap, path::PathBuf};

const LAYER: usize = 30;

struct Slot {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<HudMaterial>,
}

#[derive(Resource, Default)]
struct Setup(bool);

#[derive(Component)]
struct Output;

#[derive(Resource)]
struct Creator {
    source: serde_json::Value,
    shapes: apt_scene::Shapes,
    custom_images: BTreeMap<String, apt_scene::Texture>,
    textures: BTreeMap<String, Handle<Image>>,
    slots: Vec<Slot>,
    target: Handle<Image>,
    composite: Handle<HudComposite>,
    rebind: bool,
    runtime: Option<Runtime>,
    next_request: u64,
    accumulator: f32,
    size: [f32; 2],
}

pub(crate) struct CreatorPlugin;
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct CreatorUpdate;
impl Plugin for CreatorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PreviewPlugin)
            .init_resource::<Setup>()
            .add_systems(
                Update,
                (setup, update.in_set(CreatorUpdate), render)
                    .chain()
                    .after(frame::PresentedPublished)
                    .in_set(frame::ClientSet::Present),
            );
    }
}

fn setup(
    mut commands: Commands,
    mut setup: ResMut<Setup>,
    mut state: ResMut<SkateCreatorState>,
    cameras: Query<(Entity, &Camera), With<crate::UiCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut composites: ResMut<Assets<HudComposite>>,
) {
    if setup.0 {
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
    setup.0 = true;
    let Some(assets) = std::env::var_os("IW4L_SKATE_ASSETS") else {
        return;
    };
    let root = PathBuf::from(assets).join("private/creator");
    if !root.join("runtime/creator.json").is_file() {
        return;
    }
    let loaded = (|| -> Result<Creator, String> {
        let source: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("runtime/creator.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if source["format"] != "iw4l-character-creator" || source["version"] != 1 {
            return Err("Unsupported creator cache. Re-run Skate data preparation.".into());
        }
        let main = source["characters"]
            .as_array()
            .ok_or("Missing creator characters")?
            .iter()
            .find(|c| c["id"] == 0)
            .ok_or("Missing creator movie")?;
        let size = [
            main["movie"]["width"]
                .as_u64()
                .ok_or("Missing creator width")? as f32,
            main["movie"]["height"]
                .as_u64()
                .ok_or("Missing creator height")? as f32,
        ];
        if size.iter().any(|v| *v < 1. || *v > 8192.) {
            return Err("Invalid creator stage".into());
        }
        let shapes: apt_scene::Shapes =
            serde_json::from_value(source["shapes"].clone()).map_err(|e| e.to_string())?;
        let custom_images: BTreeMap<String, apt_scene::Texture> =
            serde_json::from_value(source["custom_images"].clone()).map_err(|e| e.to_string())?;
        let library =
            assets::character::local_library().ok_or("Missing full character catalogue")?;
        let runtime = Runtime::load(&source, library.default_profile(true)?)?;
        let mut files = BTreeMap::new();
        for texture in shapes
            .values()
            .flatten()
            .filter_map(|shape| shape.texture.as_ref())
            .chain(custom_images.values())
        {
            files.insert(texture.rgba.clone(), [texture.width, texture.height]);
        }
        for font in runtime.bindings.movie.text_assets.fonts.values() {
            files.insert(font.texture.clone(), font.size);
        }
        let mut textures = BTreeMap::new();
        files.insert(String::new(), [1, 1]);
        for (path, size) in files {
            let bytes = if path.is_empty() {
                vec![255; 4]
            } else {
                if PathBuf::from(&path)
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
                {
                    return Err("Invalid creator texture path".into());
                }
                std::fs::read(root.join(&path)).map_err(|e| format!("Creator {path}: {e}"))?
            };
            if size.contains(&0)
                || bytes.len() as u64 != u64::from(size[0]) * u64::from(size[1]) * 4
            {
                return Err(format!("Invalid creator texture size: {path}"));
            }
            textures.insert(
                path,
                images.add(Image::new(
                    Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    bytes,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::RENDER_WORLD,
                )),
            );
        }
        Ok(Creator {
            source,
            shapes,
            custom_images,
            textures,
            slots: Vec::new(),
            target: Handle::default(),
            composite: Handle::default(),
            rebind: false,
            runtime: None,
            next_request: 1,
            accumulator: 0.,
            size,
        })
    })();
    match loaded {
        Ok(mut creator) => {
            let target = images.add(Image::new_target_texture(
                window.physical_width().max(1),
                window.physical_height().max(1),
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            creator.target = target.clone();
            commands.spawn((
                Camera2d,
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: creator.size[0],
                        height: creator.size[1],
                    },
                    ..OrthographicProjection::default_2d()
                }),
                Camera {
                    order: -99,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(target.clone().into()),
                RenderLayers::layer(LAYER),
                Msaa::Off,
            ));
            creator.composite = composites.add(HudComposite { image: target });
            commands.spawn((
                Output,
                MaterialNode(creator.composite.clone()),
                UiTargetCamera(output),
                GlobalZIndex(10),
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
            ));
            commands.insert_resource(creator);
            state.available = true;
            diag::info!(
                World,
                "Original Skate character creator loaded from {}",
                root.display()
            );
        }
        Err(error) => diag::warn!(
            World,
            "Original Skate character creator could not load: {error}"
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn update(
    creator: Option<ResMut<Creator>>,
    mut state: ResMut<SkateCreatorState>,
    character: Res<net::LocalCharacter>,
    time: Res<Time>,
    mut inputs: MessageReader<UiCharacterInput>,
    mut results: MessageReader<UiCharacterEditResult>,
    mut edits: MessageWriter<UiCharacterEdit>,
    mut menus: MessageWriter<UiMenuRequest>,
    cameras: Query<(Entity, &Camera), With<crate::UiCamera>>,
    mut outputs: Query<&mut UiTargetCamera, With<Output>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut composites: ResMut<Assets<HudComposite>>,
    mut preview: ResMut<PreviewState>,
) {
    let input: Vec<_> = inputs.read().map(|message| message.key).collect();
    let results: Vec<_> = results.read().cloned().collect();
    let Some(mut creator) = creator else {
        return;
    };
    if let Some(camera) = cameras
        .iter()
        .filter(|(_, c)| c.is_active)
        .max_by_key(|(e, c)| (c.order, e.to_bits()))
        .map(|(e, _)| e)
    {
        for mut output in &mut outputs {
            output.0 = camera;
        }
    }
    if creator.rebind
        && let Some(mut material) = composites.get_mut(&creator.composite)
    {
        material.image = creator.target.clone();
    }
    creator.rebind = false;
    let size = Extent3d {
        width: window.physical_width().max(1),
        height: window.physical_height().max(1),
        depth_or_array_layers: 1,
    };
    if let Some(mut image) = images.get_mut(&creator.target)
        && image.texture_descriptor.size != size
    {
        image.resize(size);
        creator.rebind = true;
        if let Some(mut material) = composites.get_mut(&creator.composite) {
            material.image = creator.target.clone();
        }
    }
    if !state.active {
        preview.profile = None;
        creator.runtime = None;
        creator.accumulator = 0.;
        return;
    }
    let result = (|| -> Result<(), String> {
        let profile = character
            .0
            .profile
            .as_deref()
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| {
                assets::character::local_library()
                    .ok_or("Missing character catalogue")?
                    .default_profile(true)
            })?;
        if creator.runtime.is_none() {
            creator.runtime = Some(Runtime::load(&creator.source, profile.clone())?);
        }
        for result in &results {
            creator
                .runtime
                .as_mut()
                .ok_or("Creator not open")?
                .acknowledge(result.request_id, &result.result, &profile)?;
        }
        creator
            .runtime
            .as_mut()
            .ok_or("Creator not open")?
            .sync_profile(&profile)?;
        for key in input {
            let key = match key {
                UiMenuKey::Up => Key::Up,
                UiMenuKey::Down => Key::Down,
                UiMenuKey::Left => Key::Left,
                UiMenuKey::Right => Key::Right,
                UiMenuKey::Enter => Key::Select,
                UiMenuKey::Escape => Key::Back,
                _ => continue,
            };
            let request_id = creator.next_request;
            if let Some(args) = creator
                .runtime
                .as_mut()
                .ok_or("Creator not open")?
                .input(key, request_id)?
            {
                creator.next_request = creator
                    .next_request
                    .checked_add(1)
                    .ok_or("Creator request limit reached")?;
                edits.write(UiCharacterEdit { request_id, args });
            }
        }
        creator.accumulator += time.delta_secs().min(0.1) * 60.;
        let frames = creator.accumulator.floor() as usize;
        creator.accumulator -= frames as f32;
        creator
            .runtime
            .as_mut()
            .ok_or("Creator not open")?
            .advance(frames)?;
        preview.profile = creator
            .runtime
            .as_ref()
            .map(|runtime| runtime.bindings.profile.clone());
        Ok(())
    })();
    if let Err(error) = result {
        diag::warn!(World, "Original Skate creator stopped: {error}");
        creator.runtime = None;
        preview.profile = None;
        state.available = false;
        state.active = false;
        menus.write(UiMenuRequest::Close(frame::SKATE_CREATOR_MENU.into()));
    } else if creator.runtime.as_ref().is_some_and(|r| r.bindings.closed) {
        creator.runtime = None;
        preview.profile = None;
        state.active = false;
        menus.write(UiMenuRequest::Close(frame::SKATE_CREATOR_MENU.into()));
    }
}

fn render(
    mut commands: Commands,
    creator: Option<ResMut<Creator>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HudMaterial>>,
    mut state: ResMut<SkateCreatorState>,
) {
    let Some(mut creator) = creator else {
        return;
    };
    let drawn = creator.runtime.as_ref().map(|runtime| {
        apt_scene::draw_with_images(
            &runtime.bindings.movie,
            &runtime.vm,
            &creator.shapes,
            &creator.custom_images,
        )
    });
    let drawn = drawn.map(|result| {
        result.and_then(|draws| {
            if let Some(draw) = draws
                .iter()
                .find(|draw| !creator.textures.contains_key(&draw.texture))
            {
                return Err(format!(
                    "Missing original creator texture: {}",
                    draw.texture
                ));
            }
            Ok(draws)
        })
    });
    let draws = match drawn {
        Some(Ok(draws)) => draws,
        Some(Err(error)) => {
            diag::warn!(World, "Original Skate creator geometry: {error}");
            commands.write_message(UiMenuRequest::Close(frame::SKATE_CREATOR_MENU.into()));
            creator.runtime = None;
            state.available = false;
            state.active = false;
            Vec::new()
        }
        None => Vec::new(),
    };
    for (index, draw) in draws.iter().enumerate() {
        let texture = creator.textures[&draw.texture].clone();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            draw.vertices
                .iter()
                .map(|v| {
                    [
                        v.position[0] - creator.size[0] * 0.5,
                        creator.size[1] * 0.5 - v.position[1],
                        0.,
                    ]
                })
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
        if index == creator.slots.len() {
            let mesh = meshes.add(mesh);
            let material = materials.add(material);
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., index as f32 * 0.01),
                    RenderLayers::layer(LAYER),
                ))
                .id();
            creator.slots.push(Slot {
                entity,
                mesh,
                material,
            });
        } else {
            let slot = &creator.slots[index];
            if let Some(mut old) = meshes.get_mut(&slot.mesh) {
                *old = mesh;
            }
            if let Some(mut old) = materials.get_mut(&slot.material) {
                *old = material;
            }
            commands.entity(slot.entity).insert(Visibility::Visible);
        }
    }
    for slot in creator.slots.iter().skip(draws.len()) {
        commands.entity(slot.entity).insert(Visibility::Hidden);
    }
}
