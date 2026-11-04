//! Photo and video output implemented in-process, with no external programs.
//! Photos: PNG, JPEG, BMP, TGA, PPM. Video/animation: AVI (MJPEG at a fixed quality or a target bitrate, or
//! uncompressed), GIF, and PNG sequences. Other containers plug in through `sinks::VideoSink`.
pub mod avi;
pub mod gif;
pub mod jpeg;
pub mod png;
pub mod simple;
pub mod sinks;
