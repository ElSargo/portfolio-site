//! Load a cubemap texture onto a cube like a skybox and cycle through different compressed texture formats

use std::f32::consts::PI;

use bevy::{
    asset::LoadState,
    input::mouse::MouseMotion,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypeUuid,
    render::{
        mesh::MeshVertexBufferLayout,
        render_asset::RenderAssets,
        render_resource::{
            AsBindGroup, AsBindGroupError, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
            BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType,
            OwnedBindingResource, PreparedBindGroup, RenderPipelineDescriptor, SamplerBindingType,
            ShaderRef, ShaderStages, SpecializedMeshPipelineError, TextureSampleType,
            TextureViewDescriptor, TextureViewDimension,
        },
        renderer::RenderDevice,
        texture::{CompressedImageFormats, FallbackImage},
    },
};

pub struct SkyBoxPlugin {}

impl Plugin for SkyBoxPlugin {
    fn build(&self, app: &mut App) {
        app.add_startup_system(setup);
        app.add_system(cycle_cubemap_asset);
        app.add_system(asset_loaded.after(cycle_cubemap_asset));
    }
}

const CUBEMAP: (&str, CompressedImageFormats) = (
    "textures/Ryfjallet_cubemap_etc2.ktx2",
    CompressedImageFormats::ETC2,
);

#[derive(Resource)]
pub struct Cubemap {
    is_loaded: bool,
    index: usize,
    image_handle: Handle<Image>,
}

pub fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    use crate::CameraController;

    // directional 'sun' light

    let skybox_handle = asset_server.load(CUBEMAP.0);

    commands.insert_resource(Cubemap {
        is_loaded: false,
        index: 0,
        image_handle: skybox_handle,
    });
}

const CUBEMAP_SWAP_DELAY: f32 = 3.0;

pub fn cycle_cubemap_asset(
    time: Res<Time>,
    mut next_swap: Local<f32>,
    mut cubemap: ResMut<Cubemap>,
    asset_server: Res<AssetServer>,
    render_device: Res<RenderDevice>,
) {
    let now = time.elapsed_seconds();
    if *next_swap == 0.0 {
        *next_swap = now + CUBEMAP_SWAP_DELAY;
        return;
    } else if now < *next_swap {
        return;
    }
    *next_swap += CUBEMAP_SWAP_DELAY;

    let supported_compressed_formats =
        CompressedImageFormats::from_features(render_device.features());

    let mut new_index = cubemap.index;
    if !supported_compressed_formats.contains(CUBEMAP.1) {
        panic!("Skipping unsupported format: {:?}", CUBEMAP)
    }

    // Skip swapping to the same texture. Useful for when ktx2, zstd, or compressed texture support
    // is missing
    if new_index == cubemap.index {
        return;
    }

    cubemap.index = new_index;
    cubemap.image_handle = asset_server.load(CUBEMAP.0);
    cubemap.is_loaded = false;
}

pub fn asset_loaded(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cubemap_materials: ResMut<Assets<CubemapMaterial>>,
    mut cubemap: ResMut<Cubemap>,
    cubes: Query<&Handle<CubemapMaterial>>,
) {
    if !cubemap.is_loaded
        && asset_server.get_load_state(cubemap.image_handle.clone_weak()) == LoadState::Loaded
    {
        info!("Swapping to {}...", CUBEMAP.0);
        let mut image = images.get_mut(&cubemap.image_handle).unwrap();
        // NOTE: PNGs do not have any metadata that could indicate they contain a cubemap texture,
        // so they appear as one texture. The following code reconfigures the texture as necessary.
        if image.texture_descriptor.array_layer_count() == 1 {
            image.reinterpret_stacked_2d_as_array(
                image.texture_descriptor.size.height / image.texture_descriptor.size.width,
            );
            image.texture_view_descriptor = Some(TextureViewDescriptor {
                dimension: Some(TextureViewDimension::Cube),
                ..default()
            });
        }

        // spawn cube
        let mut updated = false;
        for handle in cubes.iter() {
            if let Some(material) = cubemap_materials.get_mut(handle) {
                updated = true;
                material.base_color_texture = Some(cubemap.image_handle.clone_weak());
            }
        }
        if !updated {
            commands.spawn(MaterialMeshBundle::<CubemapMaterial> {
                mesh: meshes.add(Mesh::from(shape::Cube { size: 10000.0 })),
                material: cubemap_materials.add(CubemapMaterial {
                    base_color_texture: Some(cubemap.image_handle.clone_weak()),
                }),
                ..default()
            });
        }

        cubemap.is_loaded = true;
    }
}

pub fn animate_light_direction(
    time: Res<Time>,
    mut query: Query<&mut Transform, With<DirectionalLight>>,
) {
    for mut transform in &mut query {
        transform.rotate_y(time.delta_seconds() * 0.5);
    }
}

use crate::camera::camera_controller;

#[derive(Debug, Clone, TypeUuid, Eq, Hash, PartialEq)]
#[uuid = "9509a0f8-3c05-48ee-a13e-a93226c7f488"]
pub struct CubemapMaterial {
    base_color_texture: Option<Handle<Image>>,
}

impl Material for CubemapMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/cubemap_unlit.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayout,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(AsBindGroup, TypeUuid, Debug, Clone, Hash, Eq, PartialEq)]
#[uuid = "11111111-1111-1111-2222-222222222222"]
struct Dummy {}

impl AsBindGroup for CubemapMaterial {
    type Data = Self;
    // type Data = ();

    fn as_bind_group(
        &self,
        layout: &BindGroupLayout,
        render_device: &RenderDevice,
        images: &RenderAssets<Image>,
        _fallback_image: &FallbackImage,
    ) -> Result<PreparedBindGroup<Self::Data>, AsBindGroupError> {
        let base_color_texture = self
            .base_color_texture
            .as_ref()
            .ok_or(AsBindGroupError::RetryNextUpdate)?;
        let image = images
            .get(base_color_texture)
            .ok_or(AsBindGroupError::RetryNextUpdate)?;
        let bind_group = render_device.create_bind_group(&BindGroupDescriptor {
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&image.texture_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&image.sampler),
                },
            ],
            label: Some("cubemap_texture_material_bind_group"),
            layout,
        });

        Ok(PreparedBindGroup {
            bind_group,
            bindings: vec![
                OwnedBindingResource::TextureView(image.texture_view.clone()),
                OwnedBindingResource::Sampler(image.sampler.clone()),
            ],
            data: Self {
                base_color_texture: None,
            },
        })
    }

    fn bind_group_layout(render_device: &RenderDevice) -> BindGroupLayout {
        render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[
                // Cubemap Base Color Texture
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::Cube,
                    },
                    count: None,
                },
                // Cubemap Base Color Texture Sampler
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: None,
        })
    }
}