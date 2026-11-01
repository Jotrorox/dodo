//! The WGSL translator behind std/gpu's `gpu.wgsl`: one WGSL module becomes
//! SPIR-V for the Vulkan backend, Metal Shading Language for Metal and GLSL
//! 4.50 for OpenGL, at compile time, through the `naga` crate.
//!
//! std/gpu numbers bindings in slots shared by every backend. In WGSL, slot
//! N is `@group(0) @binding(N)` for buffers, sampled textures and storage
//! textures, and the sampler read together with the sampled texture in slot
//! N is `@group(1) @binding(N)`. Vertex inputs `@location(N)` are std/gpu's
//! attribute locations. Each backend then sees the slot its own way:
//!
//! - SPIR-V: std/gpu's Vulkan backend pushes one descriptor set 0 with a
//!   combined image sampler at binding N for a sampled slot, so samplers move
//!   to set 0 and alias that descriptor together with their texture, as
//!   Vulkan allows for combined image samplers.
//! - MSL: `[[buffer(N)]]`, `[[texture(N)]]` and `[[sampler(N)]]`; vertex
//!   inputs stay `[[stage_in]]` attributes, since std/gpu describes vertex
//!   buffers with an MTLVertexDescriptor.
//! - GLSL: `layout(binding = N)`, with each texture and its sampler combined
//!   into one sampler uniform. Each entry point is wrapped in
//!   `#if defined(DODO_ENTRY_<name>)`, which std/gpu's OpenGL backend defines
//!   for the entry it compiles.
//!
//! Clip space is left as WGSL defines it (y up, depth 0 to 1): std/gpu's
//! Vulkan backend flips its viewport and its OpenGL backend uses
//! `glClipControl(GL_UPPER_LEFT, GL_ZERO_TO_ONE)`, so no backend needs the
//! coordinates adjusted.

/// std/gpu's binding slots per pipeline (`hal.MAX_SLOTS`).
pub const MAX_SLOTS: u32 = 16;

/// The Metal buffer index of naga's buffer-size table, past std/gpu's
/// binding slots (0 to 15) and vertex buffers (16 to 23). std/gpu never
/// binds it: shaders that would read it are refused.
pub const METAL_SIZES_BUFFER: u8 = 30;

/// A WGSL module in every backend's language.
#[derive(Debug, Clone)]
pub struct Translation {
    pub spirv: Vec<u32>,
    pub msl: String,
    pub glsl: String,
}

/// Why a module could not be translated; `span` is a byte range of the
/// source when the error has a place in it.
#[derive(Debug, Clone)]
pub struct Error {
    pub message: String,
    pub span: Option<(usize, usize)>,
    // The entry point the error concerns, whose name the span is moved to.
    #[cfg_attr(not(feature = "wgsl"), allow(dead_code))]
    entry: Option<String>,
}

impl Error {
    fn new(message: impl Into<String>, span: Option<(usize, usize)>) -> Self {
        Error {
            message: message.into(),
            span,
            entry: None,
        }
    }

    #[cfg(feature = "wgsl")]
    fn entry(message: impl Into<String>, name: &str) -> Self {
        Error {
            message: message.into(),
            span: None,
            entry: Some(name.to_owned()),
        }
    }

    /// Point an entry point's error at the entry point's name in `source`.
    #[cfg(feature = "wgsl")]
    fn locate(mut self, source: &str) -> Self {
        let Some(name) = self.entry.take() else {
            return self;
        };
        let bytes = source.as_bytes();
        let identifier = |byte: u8| byte == b'_' || byte.is_ascii_alphanumeric();
        let mut from = 0;
        while let Some(found) = source[from..].find(name.as_str()) {
            let start = from + found;
            let end = start + name.len();
            let before = source[..start].trim_end();
            let separated = before.len() < start;
            if separated
                && before.ends_with("fn")
                && !before[..before.len() - 2]
                    .bytes()
                    .last()
                    .is_some_and(identifier)
                && !bytes.get(end).copied().is_some_and(identifier)
            {
                self.span = Some((start, end));
                break;
            }
            from = end;
        }
        self
    }
}

#[cfg(not(feature = "wgsl"))]
pub fn translate(_source: &str) -> Result<Translation, Error> {
    Err(Error::new(
        "this Dodo compiler was built without WGSL support (the `wgsl` feature)",
        None,
    ))
}

#[cfg(feature = "wgsl")]
pub fn translate(source: &str) -> Result<Translation, Error> {
    imp::translate(source)
}

#[cfg(feature = "wgsl")]
mod imp {
    use super::{Error, MAX_SLOTS, METAL_SIZES_BUFFER, Translation};
    use naga::back::{glsl, msl, spv};
    use naga::valid::{Capabilities, ModuleInfo, ValidationFlags, Validator};
    use naga::{AddressSpace, Module, ShaderStage, Span, TypeInner};
    use std::fmt::Write;

    fn range(span: Span) -> Option<(usize, usize)> {
        span.to_range().map(|range| (range.start, range.end))
    }

    pub fn translate(source: &str) -> Result<Translation, Error> {
        let module = naga::front::wgsl::parse_str(source).map_err(|error| {
            let span = error.labels().next().and_then(|(span, _)| range(span));
            Error::new(error.message().to_owned(), span)
        })?;
        let info = Validator::new(ValidationFlags::all(), Capabilities::default())
            .validate(&module)
            .map_err(|error| {
                let span = error.spans().next().and_then(|(span, _)| range(*span));
                let mut message = error.as_inner().to_string();
                let mut cause = std::error::Error::source(error.as_inner());
                while let Some(inner) = cause {
                    write!(message, ": {inner}").unwrap();
                    cause = inner.source();
                }
                Error::new(message, span)
            })?;
        check_bindings(&module, &info).map_err(|error| error.locate(source))?;
        check_entry_points(&module).map_err(|error| error.locate(source))?;
        let msl = write_msl(&module, &info).map_err(|error| error.locate(source))?;
        let glsl = write_glsl(&module, &info).map_err(|error| error.locate(source))?;
        let spirv = write_spirv(&module, &info)?;
        Ok(Translation { spirv, msl, glsl })
    }

    fn is_sampler(module: &Module, ty: naga::Handle<naga::Type>) -> bool {
        matches!(module.types[ty].inner, TypeInner::Sampler { .. })
    }

    /// Whether `ty` is a texture that is sampled, rather than a storage one.
    fn is_sampled_image(module: &Module, ty: naga::Handle<naga::Type>) -> bool {
        matches!(
            module.types[ty].inner,
            TypeInner::Image {
                class: naga::ImageClass::Sampled { .. } | naga::ImageClass::Depth { .. },
                ..
            }
        )
    }

    /// Enforce std/gpu's slot convention: every binding in range and in
    /// its group, and within each entry point, one resource per slot and a
    /// sampled texture beside each sampler.
    fn check_bindings(module: &Module, info: &ModuleInfo) -> Result<(), Error> {
        for (handle, global) in module.global_variables.iter() {
            let span = range(module.global_variables.get_span(handle));
            if global.space == AddressSpace::Immediate {
                return Err(Error::new(
                    "std/gpu has no immediate data; put the values in a uniform buffer",
                    span,
                ));
            }
            let Some(binding) = &global.binding else {
                continue;
            };
            if binding.binding >= MAX_SLOTS {
                return Err(Error::new(
                    format!("std/gpu has binding slots 0 to {}", MAX_SLOTS - 1),
                    span,
                ));
            }
            if is_sampler(module, global.ty) {
                if binding.group != 1 {
                    return Err(Error::new(
                        "a sampler belongs in @group(1), at the @binding of the texture in @group(0) it is sampled with",
                        span,
                    ));
                }
            } else if binding.group != 0 {
                return Err(Error::new(
                    "std/gpu binds buffers and textures in @group(0); only samplers go in @group(1)",
                    span,
                ));
            }
        }
        for (index, entry) in module.entry_points.iter().enumerate() {
            let used = info.get_entry_point(index);
            let mut slots = [None::<bool>; MAX_SLOTS as usize];
            let mut samplers = Vec::new();
            for (handle, global) in module.global_variables.iter() {
                let Some(binding) = &global.binding else {
                    continue;
                };
                if used[handle].is_empty() {
                    continue;
                }
                let span = range(module.global_variables.get_span(handle));
                if is_sampler(module, global.ty) {
                    samplers.push((binding.binding, span));
                    continue;
                }
                let slot = &mut slots[binding.binding as usize];
                if slot.is_some() {
                    return Err(Error::new(
                        format!(
                            "entry point `{}` uses binding slot {} twice",
                            entry.name, binding.binding
                        ),
                        span,
                    ));
                }
                *slot = Some(is_sampled_image(module, global.ty));
            }
            for (slot, span) in samplers {
                if slots[slot as usize] != Some(true) {
                    return Err(Error::new(
                        format!(
                            "the sampler at @group(1) @binding({slot}) needs a sampled texture at @group(0) @binding({slot}) in entry point `{}`",
                            entry.name
                        ),
                        span,
                    ));
                }
            }
        }
        Ok(())
    }

    /// std/gpu finds entry points by name on every backend: the names must
    /// be plain ASCII identifiers that every target language accepts.
    fn check_entry_points(module: &Module) -> Result<(), Error> {
        if module.entry_points.is_empty() {
            return Err(Error::new(
                "the shader has no entry point (@vertex, @fragment or @compute)",
                None,
            ));
        }
        for entry in &module.entry_points {
            let ascii = entry.name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            });
            if !ascii || entry.name.len() > 200 {
                return Err(Error::entry(
                    format!(
                        "entry point `{}` needs an ASCII name of at most 200 letters, digits and underscores",
                        entry.name
                    ),
                    &entry.name,
                ));
            }
            if !matches!(
                entry.stage,
                ShaderStage::Vertex | ShaderStage::Fragment | ShaderStage::Compute
            ) {
                return Err(Error::entry(
                    format!(
                        "entry point `{}`: std/gpu runs vertex, fragment and compute shaders only",
                        entry.name
                    ),
                    &entry.name,
                ));
            }
        }
        Ok(())
    }

    fn write_spirv(module: &Module, info: &ModuleInfo) -> Result<Vec<u32>, Error> {
        // Samplers alias their texture's combined image sampler in set 0.
        let mut binding_map = spv::BindingMap::default();
        for (_, global) in module.global_variables.iter() {
            if let Some(binding) = &global.binding {
                let target = spv::BindingInfo {
                    descriptor_set: 0,
                    binding: binding.binding,
                    binding_array_size: None,
                };
                binding_map.insert(*binding, target);
            }
        }
        let options = spv::Options {
            lang_version: (1, 3),
            flags: spv::WriterFlags::LABEL_VARYINGS
                | spv::WriterFlags::CLAMP_FRAG_DEPTH
                | spv::WriterFlags::FORCE_POINT_SIZE,
            fake_missing_bindings: false,
            binding_map,
            ..spv::Options::default()
        };
        spv::write_vec(module, info, &options, None)
            .map_err(|error| Error::new(format!("cannot write SPIR-V: {error}"), None))
    }

    fn write_msl(module: &Module, info: &ModuleInfo) -> Result<String, Error> {
        let mut options = msl::Options {
            lang_version: (2, 1),
            fake_missing_bindings: false,
            zero_initialize_workgroup_memory: true,
            ..msl::Options::default()
        };
        for (index, entry) in module.entry_points.iter().enumerate() {
            let used = info.get_entry_point(index);
            let mut resources = msl::BindingMap::default();
            for (handle, global) in module.global_variables.iter() {
                let Some(binding) = &global.binding else {
                    continue;
                };
                if used[handle].is_empty() {
                    continue;
                }
                let slot = binding.binding as u8;
                let target = if is_sampler(module, global.ty) {
                    msl::BindTarget {
                        sampler: Some(msl::BindSamplerTarget::Resource(slot)),
                        ..Default::default()
                    }
                } else if matches!(module.types[global.ty].inner, TypeInner::Image { .. }) {
                    msl::BindTarget {
                        texture: Some(slot),
                        ..Default::default()
                    }
                } else {
                    msl::BindTarget {
                        buffer: Some(slot),
                        mutable: matches!(global.space, AddressSpace::Storage { .. }),
                        ..Default::default()
                    }
                };
                resources.insert(*binding, target);
            }
            options.per_entry_point_map.insert(
                entry.name.clone(),
                msl::EntryPointResources {
                    resources,
                    immediates_buffer: None,
                    // naga declares a table of buffer sizes for every entry
                    // point with a runtime-sized array, but reads it only
                    // for arrayLength, refused below.
                    sizes_buffer: Some(METAL_SIZES_BUFFER),
                },
            );
        }
        let pipeline = msl::PipelineOptions::default();
        let (text, translation) =
            msl::write_string(module, info, &options, &pipeline).map_err(|error| {
                Error::new(
                    format!("cannot write Metal Shading Language: {error}"),
                    None,
                )
            })?;
        for (entry, name) in module
            .entry_points
            .iter()
            .zip(&translation.entry_point_names)
        {
            match name {
                Ok(name) if name == &entry.name => {}
                Ok(_) => {
                    return Err(Error::entry(
                        format!(
                            "entry point `{}` is a reserved name in Metal; rename it",
                            entry.name
                        ),
                        &entry.name,
                    ));
                }
                Err(error) => {
                    return Err(Error::entry(
                        format!("entry point `{}` cannot run on Metal: {error}", entry.name),
                        &entry.name,
                    ));
                }
            }
        }
        if text.contains("_buffer_sizes.") {
            return Err(Error::new(
                "arrayLength is not available in std/gpu shaders; pass the length in a uniform buffer",
                None,
            ));
        }
        Ok(text)
    }

    fn write_glsl(module: &Module, info: &ModuleInfo) -> Result<String, Error> {
        let mut binding_map = glsl::BindingMap::default();
        for (_, global) in module.global_variables.iter() {
            if let Some(binding) = &global.binding {
                binding_map.insert(*binding, binding.binding as u8);
            }
        }
        let options = glsl::Options {
            version: glsl::Version::Desktop(450),
            writer_flags: glsl::WriterFlags::FORCE_POINT_SIZE,
            binding_map,
            zero_initialize_workgroup_memory: true,
        };
        let mut out = String::from("#version 450 core\n");
        for entry in &module.entry_points {
            let pipeline = glsl::PipelineOptions {
                shader_stage: entry.stage,
                entry_point: entry.name.clone(),
                multiview: None,
            };
            let mut text = String::new();
            glsl::Writer::new(
                &mut text,
                module,
                info,
                &options,
                &pipeline,
                naga::proc::BoundsCheckPolicies::default(),
            )
            .and_then(|mut writer| writer.write())
            .map_err(|error| {
                Error::entry(
                    format!("entry point `{}` cannot run on OpenGL: {error}", entry.name),
                    &entry.name,
                )
            })?;
            writeln!(out, "#if defined(DODO_ENTRY_{})", entry.name).unwrap();
            // Drop the per-entry #version line; the combined source has one.
            for line in text.lines() {
                if !line.starts_with("#version") {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            out.push_str("#endif\n");
        }
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const SAMPLED: &str = r#"
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(2) var image: texture_2d<f32>;
@group(1) @binding(2) var nearest: sampler;
@group(0) @binding(0) var<uniform> tint: vec4<f32>;

@vertex
fn screen_vertex(@builtin(vertex_index) id: u32) -> Out {
    let corner = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var out: Out;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

@fragment
fn texture_fragment(in: Out) -> @location(0) vec4<f32> {
    return textureSample(image, nearest, in.uv) * tint;
}
"#;

        /// The (set, binding) decorations of every variable in SPIR-V.
        fn decorations(words: &[u32]) -> Vec<(u32, u32, u32)> {
            let mut sets = std::collections::BTreeMap::new();
            let mut bindings = std::collections::BTreeMap::new();
            let mut i = 5;
            while i < words.len() {
                let count = (words[i] >> 16) as usize;
                let opcode = words[i] & 0xffff;
                // OpDecorate target DescriptorSet(34) / Binding(33) value
                if opcode == 71 && count == 4 {
                    match words[i + 2] {
                        34 => {
                            sets.insert(words[i + 1], words[i + 3]);
                        }
                        33 => {
                            bindings.insert(words[i + 1], words[i + 3]);
                        }
                        _ => {}
                    }
                }
                i += count.max(1);
            }
            bindings
                .into_iter()
                .map(|(id, binding)| (id, sets[&id], binding))
                .collect()
        }

        #[test]
        fn samplers_alias_their_texture_in_spirv() {
            let translation = translate(SAMPLED).unwrap();
            let mut found: Vec<(u32, u32)> = decorations(&translation.spirv)
                .into_iter()
                .map(|(_, set, binding)| (set, binding))
                .collect();
            found.sort();
            assert_eq!(found, vec![(0, 0), (0, 2), (0, 2)]);
        }

        #[test]
        fn metal_uses_the_slots() {
            let msl = translate(SAMPLED).unwrap().msl;
            assert!(msl.contains("[[texture(2)]]"), "{msl}");
            assert!(msl.contains("[[sampler(2)]]"), "{msl}");
            assert!(msl.contains("[[buffer(0)]]"), "{msl}");
            assert!(msl.contains("texture_fragment("), "{msl}");
        }

        #[test]
        fn glsl_wraps_each_entry_point() {
            let glsl = translate(SAMPLED).unwrap().glsl;
            assert!(glsl.starts_with("#version 450 core\n"), "{glsl}");
            assert_eq!(glsl.matches("#version").count(), 1, "{glsl}");
            let vertex = glsl.find("#if defined(DODO_ENTRY_screen_vertex)").unwrap();
            let fragment = glsl
                .find("#if defined(DODO_ENTRY_texture_fragment)")
                .unwrap();
            assert!(vertex < fragment);
            assert!(glsl.contains("binding = 2"), "{glsl}");
            assert!(glsl.contains("binding = 0"), "{glsl}");
            assert_eq!(glsl.matches("#endif").count(), 2, "{glsl}");
        }

        #[test]
        fn conventions_are_enforced() {
            let cases = [
                (
                    "@group(0) @binding(0) var s: sampler;\n@compute @workgroup_size(1) fn main() { _ = s; }",
                    "@group(1)",
                ),
                (
                    "@group(2) @binding(0) var<uniform> u: vec4<f32>;\n@compute @workgroup_size(1) fn go() { _ = u; }",
                    "@group(0)",
                ),
                (
                    "@group(0) @binding(16) var<uniform> u: vec4<f32>;\n@compute @workgroup_size(1) fn go() { _ = u; }",
                    "slots 0 to 15",
                ),
                (
                    "@group(1) @binding(3) var s: sampler;\n@compute @workgroup_size(1) fn go() { _ = s; }",
                    "needs a sampled texture",
                ),
                ("fn helper() {}", "no entry point"),
                (
                    "@group(0) @binding(0) var<storage, read_write> v: array<u32>;\n@compute @workgroup_size(1) fn go() { v[0] = arrayLength(&v); }",
                    "arrayLength",
                ),
                (
                    "@group(0) @binding(0) var<uniform> a: vec4<f32>;\n@group(0) @binding(0) var<uniform> b: vec4<f32>;\n@compute @workgroup_size(1) fn go() { _ = a; _ = b; }",
                    "conflict",
                ),
                (
                    "@compute @workgroup_size(1) fn main() {}",
                    "reserved name in Metal",
                ),
            ];
            for (source, expected) in cases {
                let error = translate(source).unwrap_err();
                assert!(
                    error.message.contains(expected),
                    "{source}: {}",
                    error.message
                );
            }
        }

        #[test]
        fn entry_points_may_share_a_slot() {
            let source = "@group(0) @binding(0) var<storage, read_write> a: array<u32>;\n\
                @group(0) @binding(0) var<storage, read_write> b: array<f32>;\n\
                @compute @workgroup_size(1) fn first() { a[0] = 1u; }\n\
                @compute @workgroup_size(1) fn second() { b[0] = 1.0; }\n";
            let glsl = translate(source).unwrap().glsl;
            assert!(glsl.contains("DODO_ENTRY_first") && glsl.contains("DODO_ENTRY_second"));
        }

        #[test]
        fn errors_point_into_the_source() {
            let source = "@compute @workgroup_size(1)\nfn go() { let x: u32 = nonsense; }\n";
            let error = translate(source).unwrap_err();
            let (start, end) = error.span.expect("a span");
            assert_eq!(&source[start..end], "nonsense", "{}", error.message);
        }
    }
}
