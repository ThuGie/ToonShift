use crate::ui::{HEIGHT, WIDTH};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::sync::mpsc::{self, Receiver};

pub struct Display {
    file: File,
    map: *mut u8,
    len: usize,
    stride: usize,
}

fn fb_info(file: &File) -> io::Result<[u32; 40]> {
    let mut info = [0u32; 40];
    // Linux FBIOGET_VSCREENINFO returns forty native-endian 32-bit fields.
    if unsafe { libc::ioctl(file.as_raw_fd(), 0x4600, info.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

impl Display {
    pub fn open() -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open("/dev/fb0")?;
        let info = fb_info(&file)?;
        let stride = fs::read_to_string("/sys/class/graphics/fb0/stride")?
            .trim()
            .parse::<usize>()
            .map_err(io::Error::other)?;
        if info[0] != WIDTH as u32
            || info[1] != HEIGHT as u32
            || info[6] != 32
            || info[8..17] != [16, 8, 0, 8, 8, 0, 0, 8, 0]
            || stride < WIDTH * 4
        {
            return Err(io::Error::other(format!(
                "Unsupported framebuffer layout: {:?}",
                &info[..20]
            )));
        }
        let len = stride * info[3] as usize;
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if map == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            file,
            map: map.cast(),
            len,
            stride,
        })
    }

    pub fn present(&self, pixels: &[u32]) -> io::Result<()> {
        let info = fb_info(&self.file)?;
        let offset = info[5] as usize * self.stride + info[4] as usize * 4;
        if offset + (HEIGHT - 1) * self.stride + WIDTH * 4 > self.len {
            return Err(io::Error::other(
                "Framebuffer page is outside mapped memory",
            ));
        }
        for (y, row) in pixels.as_chunks::<WIDTH>().0.iter().enumerate() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    row.as_ptr().cast::<u8>(),
                    self.map.add(offset + y * self.stride),
                    WIDTH * 4,
                );
            }
        }
        Ok(())
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.map.cast(), self.len);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Axis {
    min: i32,
    max: i32,
}

impl Axis {
    fn read(file: &File, code: u32) -> io::Result<Self> {
        let mut abs = [0i32; 6];
        let request = 0x80184540u32 + code; // EVIOCGABS(code), struct input_absinfo.
        if unsafe { libc::ioctl(file.as_raw_fd(), request as _, abs.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if abs[2] <= abs[1] {
            return Err(io::Error::other("Invalid touch axis range"));
        }
        Ok(Self {
            min: abs[1],
            max: abs[2],
        })
    }

    fn scale(self, value: i32, size: usize) -> usize {
        (value.clamp(self.min, self.max) - self.min) as usize * (size - 1)
            / (self.max - self.min) as usize
    }
}

pub fn touchscreen() -> io::Result<Receiver<io::Result<(usize, usize)>>> {
    let mut file = File::open("/dev/input/event1")?;
    let (x, y, multi) = match (Axis::read(&file, 0), Axis::read(&file, 1)) {
        (Ok(x), Ok(y)) => (x, y, false),
        _ => (Axis::read(&file, 53)?, Axis::read(&file, 54)?, true),
    };
    eprintln!("Touchscreen ranges {x:?} {y:?}, multitouch axes {multi}");
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        // The kernel input_event ABI uses two native longs for the timestamp.
        let mut event = [0u8; 2 * std::mem::size_of::<libc::c_long>() + 8];
        let mut down = false;
        let mut pressed = false;
        let mut position = (None, None);
        let mut primary_slot = true;
        loop {
            if let Err(error) = file.read_exact(&mut event) {
                let _ = send.send(Err(error));
                break;
            }
            let data = &event[event.len() - 8..];
            let kind = u16::from_ne_bytes([data[0], data[1]]);
            let code = u16::from_ne_bytes([data[2], data[3]]);
            let value = i32::from_ne_bytes(data[4..].try_into().unwrap());
            match (kind, code, multi) {
                (0, 3, _) => {
                    down = false;
                    pressed = false;
                    position = (None, None);
                }
                (1, 330, false) => pressed = value != 0,
                (3, 47, true) => primary_slot = value == 0,
                (3, 57, true) if primary_slot => pressed = value >= 0,
                (3, 0, false) => position.0 = Some(value),
                (3, 1, false) => position.1 = Some(value),
                (3, 53, true) if primary_slot => position.0 = Some(value),
                (3, 54, true) if primary_slot => position.1 = Some(value),
                (0, 0, _) => {
                    if down && !pressed {
                        if let (Some(px), Some(py)) = position {
                            if send
                                .send(Ok((x.scale(px, WIDTH), y.scale(py, HEIGHT))))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    down = pressed;
                }
                _ => {}
            }
        }
    });
    Ok(receive)
}

pub fn time() -> (u8, String) {
    format_time(unsafe { libc::time(std::ptr::null_mut()) }.into())
}

pub fn format_time(seconds: i64) -> (u8, String) {
    let seconds = seconds as _;
    unsafe {
        let mut local: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&seconds, &mut local).is_null() {
            return (12, "--:--".into());
        }
        (
            local.tm_hour as u8,
            format!("{:02}:{:02}", local.tm_hour, local.tm_min),
        )
    }
}
