//! AVI container writer: Motion-JPEG (fixed quality or target bitrate) or uncompressed 24-bit DIB frames.
//! Requires `Write + Seek`: RIFF chunk sizes, the frame count and the `idx1` index are patched in when finishing.
use super::jpeg::{encode_jpeg, encode_jpeg_for_size_hint};
use std::io::{self, Seek, SeekFrom, Write};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AviCodec {
    /// JPEG quality, 1..=100.
    MjpegQuality(u8),
    /// Target video stream bitrate in bits per second. The JPEG quality is chosen per frame.
    MjpegBitrate(u32),
    /// Uncompressed (BI_RGB, 24-bit): best quality, very large files.
    Raw,
}

pub struct AviEncoder<W: Write + Seek> {
    out: W,
    w: u32,
    h: u32,
    fps_num: u32,
    fps_den: u32,
    codec: AviCodec,
    frames: u32,
    max_frame: u32,
    index: Vec<(u32, u32)>, // (offset from 'movi', size)
    movi_start: u64,        // position of the 'movi' fourcc
    pos_riff_size: u64,
    pos_avih_frames: u64,
    pos_avih_buf: u64,
    pos_strh_len: u64,
    pos_strh_buf: u64,
    pos_strf_img: u64,
    pos_movi_size: u64,
    last_q: u8,
    total_bytes: u64,
}

fn u32le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

impl<W: Write + Seek> AviEncoder<W> {
    /// Frame rate is fps_num / fps_den (for example 24000/1001 for 23.976 fps).
    pub fn new(mut out: W, w: u32, h: u32, fps_num: u32, fps_den: u32, codec: AviCodec) -> io::Result<Self> {
        assert!(w > 0 && h > 0 && fps_num > 0 && fps_den > 0);
        let raw = codec == AviCodec::Raw;
        let handler: &[u8; 4] = if raw { b"DIB " } else { b"MJPG" };
        let us_per_frame = (1_000_000u64 * fps_den as u64 / fps_num as u64) as u32;
        let mut e = Self {
            out: {
                out.seek(SeekFrom::Start(0))?;
                out
            },
            w,
            h,
            fps_num,
            fps_den,
            codec,
            frames: 0,
            max_frame: 0,
            index: vec![],
            movi_start: 0,
            pos_riff_size: 0,
            pos_avih_frames: 0,
            pos_avih_buf: 0,
            pos_strh_len: 0,
            pos_strh_buf: 0,
            pos_strf_img: 0,
            pos_movi_size: 0,
            last_q: 75,
            total_bytes: 0,
        };
        let o = &mut e.out;
        o.write_all(b"RIFF")?;
        e.pos_riff_size = o.stream_position()?;
        o.write_all(&u32le(0))?;
        o.write_all(b"AVI ")?;
        // hdrl
        o.write_all(b"LIST")?;
        let hdrl_size_pos = o.stream_position()?;
        o.write_all(&u32le(0))?;
        let hdrl_start = o.stream_position()?;
        o.write_all(b"hdrl")?;
        // avih (56 bytes)
        o.write_all(b"avih")?;
        o.write_all(&u32le(56))?;
        o.write_all(&u32le(us_per_frame))?;
        o.write_all(&u32le(0))?; // max bytes/sec (patched in finish)
        o.write_all(&u32le(0))?; // padding
        o.write_all(&u32le(0x10))?; // AVIF_HASINDEX
        e.pos_avih_frames = o.stream_position()?;
        o.write_all(&u32le(0))?; // total frames
        o.write_all(&u32le(0))?; // initial frames
        o.write_all(&u32le(1))?; // streams
        e.pos_avih_buf = o.stream_position()?;
        o.write_all(&u32le(0))?; // suggested buffer
        o.write_all(&u32le(w))?;
        o.write_all(&u32le(h))?;
        o.write_all(&[0u8; 16])?;
        // strl
        o.write_all(b"LIST")?;
        let strl_size_pos = o.stream_position()?;
        o.write_all(&u32le(0))?;
        let strl_start = o.stream_position()?;
        o.write_all(b"strl")?;
        // strh (56)
        o.write_all(b"strh")?;
        o.write_all(&u32le(56))?;
        o.write_all(b"vids")?;
        o.write_all(handler)?;
        o.write_all(&u32le(0))?; // flags
        o.write_all(&[0, 0, 0, 0])?; // priority, language
        o.write_all(&u32le(0))?; // initial frames
        o.write_all(&u32le(fps_den))?; // scale
        o.write_all(&u32le(fps_num))?; // rate
        o.write_all(&u32le(0))?; // start
        e.pos_strh_len = o.stream_position()?;
        o.write_all(&u32le(0))?; // length (in frames)
        e.pos_strh_buf = o.stream_position()?;
        o.write_all(&u32le(0))?; // suggested buffer
        o.write_all(&u32le(0xFFFF_FFFF))?; // quality
        o.write_all(&u32le(0))?; // sample size
        o.write_all(&0u16.to_le_bytes())?;
        o.write_all(&0u16.to_le_bytes())?;
        o.write_all(&(w as u16).to_le_bytes())?;
        o.write_all(&(h as u16).to_le_bytes())?;
        // strf = BITMAPINFOHEADER (40)
        o.write_all(b"strf")?;
        o.write_all(&u32le(40))?;
        o.write_all(&u32le(40))?;
        o.write_all(&(w as i32).to_le_bytes())?;
        o.write_all(&(h as i32).to_le_bytes())?;
        o.write_all(&1u16.to_le_bytes())?;
        o.write_all(&24u16.to_le_bytes())?;
        o.write_all(if raw { &[0u8; 4] } else { b"MJPG" })?;
        e.pos_strf_img = o.stream_position()?;
        o.write_all(&u32le(0))?; // size image
        o.write_all(&[0u8; 16])?;
        // patch the strl/hdrl chunk sizes
        let end = o.stream_position()?;
        o.seek(SeekFrom::Start(strl_size_pos))?;
        o.write_all(&u32le((end - strl_start) as u32))?;
        o.seek(SeekFrom::Start(hdrl_size_pos))?;
        o.write_all(&u32le((end - hdrl_start) as u32))?;
        o.seek(SeekFrom::Start(end))?;
        // movi
        o.write_all(b"LIST")?;
        e.pos_movi_size = o.stream_position()?;
        o.write_all(&u32le(0))?;
        e.movi_start = o.stream_position()?;
        o.write_all(b"movi")?;
        Ok(e)
    }

    pub fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        assert_eq!(rgba.len(), self.w as usize * self.h as usize * 4, "frame size does not match the encoder dimensions");
        let (data, tag): (Vec<u8>, &[u8; 4]) = match self.codec {
            AviCodec::MjpegQuality(q) => (encode_jpeg(self.w, self.h, rgba, q), b"00dc"),
            AviCodec::MjpegBitrate(bps) => {
                let budget = (bps as u64 * self.fps_den as u64 / self.fps_num as u64 / 8).max(1) as usize;
                let (j, q) = encode_jpeg_for_size_hint(self.w, self.h, rgba, budget, self.last_q);
                self.last_q = q;
                (j, b"00dc")
            }
            AviCodec::Raw => {
                // BI_RGB: bottom-up rows, BGR byte order, each row padded to a 4-byte boundary
                let row = ((self.w as usize * 3 + 3) / 4) * 4;
                let mut d = Vec::with_capacity(row * self.h as usize);
                for y in (0..self.h as usize).rev() {
                    let start = d.len();
                    for x in 0..self.w as usize {
                        let p = &rgba[(y * self.w as usize + x) * 4..];
                        d.extend([p[2], p[1], p[0]]);
                    }
                    d.resize(start + row, 0);
                }
                (d, b"00db")
            }
        };
        let off = (self.out.stream_position()? - self.movi_start) as u32;
        self.out.write_all(tag)?;
        self.out.write_all(&u32le(data.len() as u32))?;
        self.out.write_all(&data)?;
        if data.len() % 2 == 1 {
            self.out.write_all(&[0])?; // pad to a 16-bit word boundary
        }
        self.index.push((off, data.len() as u32));
        self.max_frame = self.max_frame.max(data.len() as u32);
        self.total_bytes += data.len() as u64;
        self.frames += 1;
        Ok(())
    }

    /// Average video stream bitrate in bits per second (meaningful after frames have been written).
    pub fn average_bitrate(&self) -> f64 {
        if self.frames == 0 {
            return 0.0;
        }
        self.total_bytes as f64 * 8.0 * self.fps_num as f64 / (self.fps_den as f64 * self.frames as f64)
    }

    pub fn finish(mut self) -> io::Result<W> {
        let raw = self.codec == AviCodec::Raw;
        let end_movi = self.out.stream_position()?;
        // idx1
        self.out.write_all(b"idx1")?;
        self.out.write_all(&u32le(self.index.len() as u32 * 16))?;
        for &(off, size) in &self.index {
            self.out.write_all(if raw { b"00db" } else { b"00dc" })?;
            self.out.write_all(&u32le(0x10))?; // AVIIF_KEYFRAME: every frame is a key frame
            self.out.write_all(&u32le(off))?;
            self.out.write_all(&u32le(size))?;
        }
        let file_end = self.out.stream_position()?;
        let patch = |o: &mut W, pos: u64, v: u32| -> io::Result<()> {
            o.seek(SeekFrom::Start(pos))?;
            o.write_all(&u32le(v))
        };
        patch(&mut self.out, self.pos_riff_size, (file_end - 8) as u32)?;
        patch(&mut self.out, self.pos_avih_frames, self.frames)?;
        patch(&mut self.out, self.pos_avih_buf, self.max_frame)?;
        patch(&mut self.out, self.pos_strh_len, self.frames)?;
        patch(&mut self.out, self.pos_strh_buf, self.max_frame)?;
        patch(&mut self.out, self.pos_strf_img, if raw { self.max_frame } else { self.max_frame })?;
        patch(&mut self.out, self.pos_movi_size, (end_movi - self.movi_start) as u32 + 4)?;
        self.out.seek(SeekFrom::Start(file_end))?;
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn riff_sizes_are_consistent() {
        let mut e = AviEncoder::new(Cursor::new(Vec::new()), 16, 16, 12, 1, AviCodec::MjpegQuality(80)).unwrap();
        for i in 0..5u8 {
            e.add_frame(&vec![i * 40; 16 * 16 * 4]).unwrap();
        }
        let out = e.finish().unwrap().into_inner();
        assert_eq!(&out[..4], b"RIFF");
        let riff = u32::from_le_bytes([out[4], out[5], out[6], out[7]]) as usize;
        assert_eq!(riff + 8, out.len());
        assert!(out.windows(4).any(|w| w == b"idx1"));
    }
    #[test]
    fn raw_frame_size() {
        let mut e = AviEncoder::new(Cursor::new(Vec::new()), 5, 3, 10, 1, AviCodec::Raw).unwrap();
        e.add_frame(&vec![9u8; 5 * 3 * 4]).unwrap();
        assert_eq!(e.total_bytes, 16 * 3); // a 15-byte row is padded to 16
    }
}
