pub mod ac97;
pub mod vfs;
use crate::kinfo;
use spin::Mutex;

static AC97_DEV: Mutex<Option<ac97::Ac97>> = Mutex::new(None);

pub fn init() {
    let audio_devices = crate::hal::hal().pci.find_audio();
    for dev in audio_devices {
        if dev.subclass == 0x01 {
            kinfo!("audio: initializing AC'97 controller at {:?}", dev.address);
            if let Some(driver) = ac97::Ac97::init(&dev) {
                *AC97_DEV.lock() = Some(driver);
                break;
            }
        }
    }
}

