use anyhow::{Context, Result};
use image::GenericImageView;

pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl Texture {
    pub fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        label: &str,
    ) -> Result<Self> {
        // 1. 用 image 库解码字节，得到 DynamicImage
        let image = image::load_from_memory(bytes).context("图片解码失败")?;

        // 2. 复用 from_image 的逻辑
        Self::from_image(device, queue, image, Some(label))
    }

    pub fn from_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: image::DynamicImage,
        label: Option<&str>,
    ) -> Result<Self> {
        // 1. 获取宽高
        let dimensions = image.dimensions();

        // 2. 转成 RGBA8 格式。这是最通用的 32 位纹理格式，带透明度
        //    wgpu 不能直接吃 DynamicImage，必须转成字节数组
        let rgba = image.to_rgba8();

        let (data, bytes_per_row) = Self::pad_rgba_to_256(
            dimensions.0, dimensions.1, &rgba
        );

        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };

        // 3. 创建 GPU 上的纹理对象
        let texture = device.create_texture(
            &wgpu::TextureDescriptor {
                label,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }
        );

        // 4. 把 CPU 上的图片数据上传到 GPU
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(dimensions.1),
            },
            size,
        );

        // 5. 创建视图 (让着色器能读取这个纹理)
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // 6. 创建采样器 (控制纹理的缩放和平铺方式)
        let sampler = device.create_sampler(
            &wgpu::SamplerDescriptor {
                label: Some("Texture Sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            }
        );

        Ok(Self {
            texture,
            view,
            sampler,
        })
    }

    fn pad_rgba_to_256(width: u32, height: u32, rgba: &[u8]) -> (Vec<u8>, u32) {
        let unpadded_bytes_per_row = width * 4;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(256) * 256;

        if unpadded_bytes_per_row == padded_bytes_per_row {
            return (rgba.to_vec(), unpadded_bytes_per_row);
        }

        let mut padded = Vec::with_capacity((padded_bytes_per_row * height) as usize);
        for chunk in rgba.chunks_exact(unpadded_bytes_per_row as usize) {
            padded.extend_from_slice(chunk);
            padded.resize(padded.len() + (padded_bytes_per_row - unpadded_bytes_per_row) as usize, 0);
        }
        (padded, padded_bytes_per_row)
    }
}