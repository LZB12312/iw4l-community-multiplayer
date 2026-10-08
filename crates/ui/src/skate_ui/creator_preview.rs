use super::creator_scene::CreatorScene;
use assets::character::CharacterMeshPart;
use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera::{
        CompositingSpace, RenderTarget,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    mesh::{Indices, MeshVertexBufferLayoutRef},
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendState, CompareFunction, PrimitiveTopology, RenderPipelineDescriptor,
        ShaderType, SpecializedMeshPipelineError, TextureFormat,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin, MeshMaterial2d},
    ui_render::UiMaterialPlugin,
    window::PrimaryWindow,
};
use sim::character::CharacterProfile;
use skate_data::creator_animations::CreatorAnimations;
use std::{collections::BTreeMap, path::Path, sync::Arc};

const LAYER: usize = 29;

#[derive(Resource, Default)]
pub(super) struct PreviewState {
    pub profile: Option<CharacterProfile>,
}

#[derive(Clone, Copy, Debug, ShaderType)]
struct PreviewUniform {
    clip_from_model: Mat4,
    light: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct PreviewMaterial {
    #[uniform(0)]
    view: PreviewUniform,
    #[texture(1)]
    #[sampler(2)]
    atlas: Handle<Image>,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct PreviewComposite {
    #[texture(0)]
    #[sampler(1)]
    image: Handle<Image>,
}

impl UiMaterial for PreviewComposite {
    fn fragment_shader() -> ShaderRef {
        "embedded://ui/skate_ui/creator_composite.wgsl".into()
    }
    fn specialize(descriptor: &mut RenderPipelineDescriptor, key: UiMaterialKey<Self>) {
        if let Some(fragment) = &mut descriptor.fragment {
            if !key.target_format.is_srgb() {
                fragment.shader_defs.push("ENCODE_SRGB".into());
            }
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
    }
}

impl Material2d for PreviewMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://ui/skate_ui/creator_preview.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        Self::vertex_shader()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(true);
            depth.depth_compare = Some(CompareFunction::GreaterEqual);
        }
        Ok(())
    }
}

struct Surface {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<PreviewMaterial>,
    part: usize,
    surface: usize,
}

#[derive(Component)]
struct Output;

#[derive(Component)]
struct PreviewCamera;

#[derive(Resource)]
struct Preview {
    animations: [CreatorAnimations; 2],
    scene: CreatorScene,
    profile: Option<CharacterProfile>,
    parts: Option<Arc<Vec<CharacterMeshPart>>>,
    surfaces: Vec<Surface>,
    textures: BTreeMap<String, Handle<Image>>,
    target: Handle<Image>,
    composite: Handle<PreviewComposite>,
    seconds: f32,
    rotation: f32,
    failed: bool,
    rebind: bool,
}

pub(super) struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "creator_preview.wgsl");
        embedded_asset!(app, "creator_composite.wgsl");
        app.add_plugins((
            Material2dPlugin::<PreviewMaterial>::default(),
            UiMaterialPlugin::<PreviewComposite>::default(),
        ))
        .init_resource::<PreviewState>()
        .add_systems(
            Update,
            update
                .after(super::creator_renderer::CreatorUpdate)
                .in_set(frame::ClientSet::Present),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    state: Res<PreviewState>,
    preview: Option<ResMut<Preview>>,
    mut attempted: Local<bool>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    cameras: Query<(Entity, &Camera), (With<crate::UiCamera>, Without<PreviewCamera>)>,
    mut preview_cameras: Query<&mut Camera, With<PreviewCamera>>,
    mut outputs: Query<(&mut UiTargetCamera, &mut Visibility), With<Output>>,
    active_pad: Res<frame::ActivePad>,
    gamepads: Query<&Gamepad>,
    keys: Res<ButtonInput<KeyCode>>,
    mut images: ResMut<Assets<Image>>,
    mut composites: ResMut<Assets<PreviewComposite>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PreviewMaterial>>,
) {
    let output_camera = cameras
        .iter()
        .filter(|(_, c)| c.is_active)
        .max_by_key(|(e, c)| (c.order, e.to_bits()))
        .map(|(e, _)| e);
    let Some(output_camera) = output_camera else {
        return;
    };
    let Some(mut preview) = preview else {
        if *attempted {
            return;
        }
        *attempted = true;
        let Some(assets) = std::env::var_os("IW4L_SKATE_ASSETS") else {
            return;
        };
        let assets = Path::new(&assets);
        let root = assets.join("private/creator/animations");
        let loaded = (|| -> Result<([CreatorAnimations; 2], CreatorScene), String> {
            let animations = [
                CreatorAnimations::load(&root.join("cac_edit_male.abin"))?,
                CreatorAnimations::load(&root.join("cac_edit_female.abin"))?,
            ];
            Ok((animations, CreatorScene::load(assets)?))
        })();
        let (animations, scene) = match loaded {
            Ok(data) => data,
            Err(error) => {
                diag::warn!(World, "Skate creator preview unavailable: {error}");
                return;
            }
        };
        let target = images.add(Image::new_target_texture(
            window.physical_width().max(1),
            window.physical_height().max(1),
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        commands.spawn((
            Camera2d,
            CompositingSpace::Linear,
            PreviewCamera,
            Camera {
                order: -100,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.08, 0.09, 0.1)),
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
            RenderLayers::layer(LAYER),
            Msaa::Off,
        ));
        let composite = composites.add(PreviewComposite {
            image: target.clone(),
        });
        commands.spawn((
            Output,
            MaterialNode(composite.clone()),
            UiTargetCamera(output_camera),
            GlobalZIndex(9),
            Pickable::IGNORE,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ));
        commands.insert_resource(Preview {
            animations,
            scene,
            profile: None,
            parts: None,
            surfaces: Vec::new(),
            textures: BTreeMap::new(),
            target,
            composite,
            seconds: 0.,
            rotation: 0.,
            failed: false,
            rebind: false,
        });
        diag::info!(World, "Skate creator standing animations loaded");
        return;
    };
    let visible = state.profile.is_some() && !preview.failed;
    for mut camera in &mut preview_cameras {
        camera.is_active = visible;
    }
    for (mut camera, mut visibility) in &mut outputs {
        camera.0 = output_camera;
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !visible {
        preview.seconds = 0.;
        preview.rotation = 0.;
        return;
    }
    let result = (|| -> Result<(), String> {
        let profile = state.profile.as_ref().ok_or("Missing preview profile")?;
        if preview.rebind {
            if let Some(mut composite) = composites.get_mut(&preview.composite) {
                composite.image = preview.target.clone();
            }
        }
        preview.rebind = false;
        if let Some(mut image) = images.get_mut(&preview.target) {
            let width = window.physical_width().max(1);
            let height = window.physical_height().max(1);
            if image.texture_descriptor.size.width != width
                || image.texture_descriptor.size.height != height
            {
                image.resize(bevy::render::render_resource::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                });
                preview.rebind = true;
                if let Some(mut composite) = composites.get_mut(&preview.composite) {
                    composite.image = preview.target.clone();
                }
            }
        }
        if preview.profile.as_ref() != Some(profile) {
            for surface in preview.surfaces.drain(..) {
                commands.entity(surface.entity).despawn();
            }
            let library =
                assets::character::local_library().ok_or("Missing preview character library")?;
            let parts = library.assemble_cached(profile)?;
            let mut used: BTreeMap<String, Handle<Image>> = BTreeMap::new();
            for (part_index, part) in parts.iter().enumerate() {
                let keys: Vec<_> = part.texture_keys().collect();
                if keys.len() != part.native.surfaces.len() {
                    return Err("Missing preview surface texture".into());
                }
                for (surface_index, (surface, key)) in
                    part.native.surfaces.iter().zip(keys).enumerate()
                {
                    let texture = match used.get(key).or_else(|| preview.textures.get(key)) {
                        Some(image) => image.clone(),
                        None => images.add(
                            part.texture_image(key)
                                .ok_or("Missing preview texture pixels")?,
                        ),
                    };
                    used.insert(key.to_owned(), texture.clone());
                    let mut mesh = Mesh::new(
                        PrimitiveTopology::TriangleList,
                        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
                    );
                    mesh.insert_indices(Indices::U32(surface.indices.clone()));
                    mesh.insert_attribute(
                        Mesh::ATTRIBUTE_UV_0,
                        surface
                            .vertices
                            .iter()
                            .map(|v| {
                                [
                                    half::f16::from_bits((v.uv >> 16) as u16).to_f32(),
                                    half::f16::from_bits(v.uv as u16).to_f32(),
                                ]
                            })
                            .collect::<Vec<_>>(),
                    );
                    mesh.insert_attribute(
                        Mesh::ATTRIBUTE_POSITION,
                        vec![[0.; 3]; surface.vertices.len()],
                    );
                    mesh.insert_attribute(
                        Mesh::ATTRIBUTE_NORMAL,
                        vec![[0., 1., 0.]; surface.vertices.len()],
                    );
                    let mesh = meshes.add(mesh);
                    let material = materials.add(PreviewMaterial {
                        view: PreviewUniform {
                            clip_from_model: Mat4::IDENTITY,
                            light: Vec4::new(-0.3, 0.8, 0.9, 0.),
                        },
                        atlas: texture,
                    });
                    let entity = commands
                        .spawn((
                            Mesh2d(mesh.clone()),
                            MeshMaterial2d(material.clone()),
                            Transform::default(),
                            RenderLayers::layer(LAYER),
                            NoFrustumCulling,
                        ))
                        .id();
                    preview.surfaces.push(Surface {
                        entity,
                        mesh,
                        material,
                        part: part_index,
                        surface: surface_index,
                    });
                }
            }
            preview.textures = used;
            preview.parts = Some(parts);
            preview.profile = Some(profile.clone());
        }
        let delta = time.delta_secs().min(0.1);
        preview.seconds += delta;
        if window.focused {
            let pad = active_pad.0.and_then(|e| gamepads.get(e).ok());
            let axis = pad.map_or(0., |p| {
                p.get(GamepadButton::RightTrigger2).unwrap_or(0.)
                    - p.get(GamepadButton::LeftTrigger2).unwrap_or(0.)
            });
            let axis = axis + u8::from(keys.pressed(KeyCode::KeyE)) as f32
                - u8::from(keys.pressed(KeyCode::KeyQ)) as f32;
            preview.rotation = (preview.rotation + axis.clamp(-1., 1.) * delta * 2.)
                .rem_euclid(std::f32::consts::TAU);
        }
        let animations = &preview.animations[usize::from(!profile.male)];
        let pose = animations.pose(
            u32::try_from(profile.posture.0).map_err(|_| "Invalid preview posture")?,
            preview.seconds,
        )?;
        let bones: Vec<_> = pose
            .iter()
            .map(|m| Mat4::from_cols_array(&std::array::from_fn(|i| m[i / 4][i % 4])))
            .collect();
        let parts = preview.parts.as_ref().ok_or("Missing preview geometry")?;
        let basis = Mat4::from_cols(Vec4::X, -Vec4::Z, Vec4::Y, Vec4::W);
        let mut part_matrices = Vec::new();
        for part in parts.iter() {
            let mut matrices = Vec::new();
            for joint in &part.native.joints {
                let index = animations
                    .names
                    .iter()
                    .position(|n| n == &joint.target)
                    .ok_or("Missing preview joint")?;
                let inverse = joint
                    .inverse_bind
                    .as_ref()
                    .ok_or("Missing preview inverse bind")?;
                matrices.push(bones[index] * basis * Mat4::from_cols_array(inverse));
            }
            part_matrices.push(matrices);
        }
        let aspect = window.physical_width().max(1) as f32 / window.physical_height().max(1) as f32;
        let clip_from_model = preview.scene.clip_from_model(aspect, preview.rotation);
        for output in &preview.surfaces {
            let surface = &parts[output.part].native.surfaces[output.surface];
            let matrices = &part_matrices[output.part];
            let mut positions = Vec::with_capacity(surface.vertices.len());
            let mut normals = Vec::with_capacity(surface.vertices.len());
            for vertex in &surface.vertices {
                let mut position = Vec3::ZERO;
                let mut normal = Vec3::ZERO;
                for lane in 0..4 {
                    if vertex.weights[lane] == 0. {
                        continue;
                    }
                    let matrix = matrices
                        .get(vertex.joints[lane])
                        .ok_or("Invalid preview vertex joint")?;
                    position +=
                        matrix.transform_point3(vertex.position.into()) * vertex.weights[lane];
                    normal += matrix.transform_vector3(vertex.normal.into()) * vertex.weights[lane];
                }
                if !position.is_finite() || !normal.is_finite() {
                    return Err("Nonfinite preview geometry".into());
                }
                positions.push(position.to_array());
                normals.push(normal.normalize_or(Vec3::Y).to_array());
            }
            let mut mesh = meshes.get_mut(&output.mesh).ok_or("Missing preview mesh")?;
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
            materials
                .get_mut(&output.material)
                .ok_or("Missing preview material")?
                .view
                .clip_from_model = clip_from_model;
        }
        Ok(())
    })();
    if let Err(error) = result {
        diag::warn!(World, "Skate creator preview stopped: {error}");
        preview.failed = true;
        for mut camera in &mut preview_cameras {
            camera.is_active = false;
        }
        for (_, mut visibility) in &mut outputs {
            *visibility = Visibility::Hidden;
        }
    }
}
