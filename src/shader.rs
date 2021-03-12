use alloc::vec::Vec;

use hashbrown::HashMap;

use crate::Renderer;

pub enum ShaderBindingType {
    UniformBuffer,
    Texture2D,
    Sampler,
}

impl ShaderBindingType {
    pub fn wgpu_type(&self) -> wgpu::BindingType {
        match self {
            ShaderBindingType::UniformBuffer => wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            ShaderBindingType::Texture2D => wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                multisampled: false,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            ShaderBindingType::Sampler => wgpu::BindingType::Sampler {
                comparison: false,
                filtering: true,
            },
        }
    }
}

pub struct ShaderBinding {
    pub(crate) binding: u32,
    pub(crate) binding_type: ShaderBindingType,
}

impl ShaderBinding {
    pub fn new(binding: u32, binding_type: ShaderBindingType) -> Self {
        Self { binding, binding_type }
    }

    pub fn wgpu_entry(&self, stage: wgpu::ShaderStage) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding: self.binding,
            visibility: stage,
            ty: self.binding_type.wgpu_type(),
            count: None,
        }
    }
}

pub struct Shader {
    pub(crate) module: wgpu::ShaderModule,
    pub(crate) entry: &'static str,
    pub(crate) bindings: HashMap<&'static str, ShaderBinding>,
    pub(crate) inputs: HashMap<&'static str, u32>,
}

impl Shader {
    pub fn new(
        renderer: &Renderer,
        bytes: &[u8],
        entry: &'static str,
        bindings: HashMap<&'static str, ShaderBinding>,
        inputs: HashMap<&'static str, u32>,
    ) -> Self {
        let spv = (0..bytes.len() / 4)
            .map(|x| u32::from_le_bytes([bytes[x * 4], bytes[x * 4 + 1], bytes[x * 4 + 2], bytes[x * 4 + 3]]))
            .collect::<Vec<_>>();
        let module = renderer.device.create_shader_module(&wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::SpirV(spv.into()),
            flags: wgpu::ShaderFlags::default(),
        });

        Self {
            module,
            entry,
            bindings,
            inputs,
        }
    }

    pub fn wgpu_bindings(&self, stage: wgpu::ShaderStage) -> Vec<wgpu::BindGroupLayoutEntry> {
        self.bindings.iter().map(|(_, x)| x.wgpu_entry(stage)).collect::<Vec<_>>()
    }
}
