use std::{ffi::CStr, path::PathBuf, ptr::null};

use anyhow::Result;
use ndi_sys as ffi;

pub struct NdiLib {
    lib_ptr: *const ffi::NDIlib_v5,
}

impl NdiLib {
    pub fn new() -> Result<Self> {
        let runtime_dir = std::env::var("NDI_RUNTIME_DIR_V5").ok();
        let lib_base_name = CStr::from_bytes_with_nul(ffi::NDILIB_LIBRARY_NAME)?.to_str()?;

        let lib_name = if let Some(d) = runtime_dir {
            PathBuf::from(d).join(lib_base_name)
        } else {
            PathBuf::from(lib_base_name)
        };

        let library_entry = unsafe {
            let lib = libloading::Library::new(lib_name)?;
            let load_fn =
                lib.get::<unsafe extern "C" fn() -> *const ffi::NDIlib_v5>(b"NDIlib_v5_load")?;
            let ptr = load_fn();
            std::mem::forget(lib);
            ptr
        };

        let init_result = unsafe { (*library_entry).__bindgen_anon_1.initialize.unwrap()() };
        if !init_result {
            return Err(anyhow::anyhow!("Failed to initialize NDI library"));
        }

        Ok(Self {
            lib_ptr: library_entry,
        })
    }

    pub fn version(&self) -> String {
        let version = unsafe { (*self.lib_ptr).__bindgen_anon_3.version.unwrap()() };
        unsafe { CStr::from_ptr(version).to_str().unwrap().to_string() }
    }

    pub fn create_sender(
        &self,
        name: Option<&str>,
        group: Option<&str>,
        clock_video: bool,
        clock_audio: bool,
    ) -> Result<Sender> {
        let name = name.map(|s| std::ffi::CString::new(s).unwrap());
        let group = group.map(|s| std::ffi::CString::new(s).unwrap());

        let param = ffi::NDIlib_send_create_t {
            p_ndi_name: name.map(|s| s.as_ptr()).unwrap_or(null()),
            p_groups: group.map(|s| s.as_ptr()).unwrap_or(null()),
            clock_video,
            clock_audio,
        };

        let sender = unsafe { (*self.lib_ptr).__bindgen_anon_9.send_create.unwrap()(&param) };
        if sender.is_null() {
            return Err(anyhow::anyhow!("Failed to create sender"));
        }

        Ok(Sender {
            lib_ptr: self.lib_ptr,
            sender_ptr: sender,
        })
    }
}

pub struct Sender {
    lib_ptr: *const ffi::NDIlib_v5,
    sender_ptr: ffi::NDIlib_send_instance_t,
}

impl Sender {
    pub fn send(&self, frame: Frame) {
        let mut frame_v2: ffi::NDIlib_video_frame_v2_t = unsafe { std::mem::zeroed() };
        frame_v2.xres = frame.width as i32;
        frame_v2.yres = frame.height as i32;
        frame_v2.FourCC = frame.format.to_fourcc();
        frame_v2.p_data = frame.data.as_mut_ptr();
        frame_v2.__bindgen_anon_1.line_stride_in_bytes = frame.stride_in_bytes as i32;
        frame_v2.timecode = ffi::NDIlib_send_timecode_synthesize;
        unsafe {
            (*self.lib_ptr)
                .__bindgen_anon_51
                .send_send_video_v2
                .unwrap()(self.sender_ptr, &frame_v2);
        }
    }

    pub fn connections_count(&self) -> u32 {
        unsafe {
            (*self.lib_ptr)
                .__bindgen_anon_18
                .send_get_no_connections
                .unwrap()(self.sender_ptr, 0) as u32
        }
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        unsafe {
            (*self.lib_ptr).__bindgen_anon_10.send_destroy.unwrap()(self.sender_ptr);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFormat {
    RGBA,
    RGBX,
    BGRA,
    BGRX,
}

impl VideoFormat {
    fn to_fourcc(self) -> ffi::NDIlib_FourCC_video_type_e {
        match self {
            VideoFormat::RGBA => ffi::NDIlib_FourCC_video_type_e_NDIlib_FourCC_type_RGBA,
            VideoFormat::RGBX => ffi::NDIlib_FourCC_video_type_e_NDIlib_FourCC_type_RGBX,
            VideoFormat::BGRA => ffi::NDIlib_FourCC_video_type_e_NDIlib_FourCC_type_BGRA,
            VideoFormat::BGRX => ffi::NDIlib_FourCC_video_type_e_NDIlib_FourCC_type_BGRX,
        }
    }
}

pub struct Frame<'a> {
    pub width: u32,
    pub height: u32,
    pub format: VideoFormat,
    pub data: &'a mut [u8],
    pub stride_in_bytes: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let n = NdiLib::new().unwrap();
        dbg!(n.version());
        let sender = n.create_sender(Some("Desktop"), None, true, true).unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}
