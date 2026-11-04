# `.jank` project format (version 1)

Little-endian. Header `"JANK" | u16 version | u16 reserved`, then chunks:
`tag[4] | u8 compression (0 raw, 1 deflate) | u32 raw_len | u32 stored_len | u32 crc32(stored) | data`, ending with an empty `END\0` chunk.

Chunks: `META` (name), `ASST` (models, textures, skeletons, emotions), `SCEN` (scene, camera, style), `ANIM` (animations), `LAYR` (optional: scene layers - pictures behind and in front of the 3D scene - and the layer stacks of textures), `LITE` (optional: time of day, sun azimuth, shadow mode, strength, quality and softness; written only when they differ from the defaults; one reserved float field is kept for older files and is always 0). Files without the optional chunks are written exactly as before. Unknown chunks are skipped, so older builds can read newer files that only add chunks. The format version is independent of the application version (see `core/src/version.rs`, `FORMAT_POLICY`). The reference implementation is `core/src/project.rs`.
