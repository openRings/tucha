use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, Host};

/// Перечисление доступных аудиоустройств
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

pub struct AudioDevices {
    host: Host,
}

impl AudioDevices {
    pub fn new() -> Self {
        Self { host: cpal::default_host() }
    }

    pub fn input_devices(&self) -> Vec<DeviceInfo> {
        let default_name = self.host
            .default_input_device()
            .and_then(|d| d.name().ok());

        self.host
            .input_devices()
            .map(|iter| {
                iter.filter_map(|d| {
                    let name = d.name().ok()?;
                    let is_default = default_name.as_deref() == Some(&name);
                    Some(DeviceInfo { name, is_default })
                })
                .collect()
            })
            .unwrap_or_default()
    }

    pub fn output_devices(&self) -> Vec<DeviceInfo> {
        let default_name = self.host
            .default_output_device()
            .and_then(|d| d.name().ok());

        self.host
            .output_devices()
            .map(|iter| {
                iter.filter_map(|d| {
                    let name = d.name().ok()?;
                    let is_default = default_name.as_deref() == Some(&name);
                    Some(DeviceInfo { name, is_default })
                })
                .collect()
            })
            .unwrap_or_default()
    }

    pub fn find_input(&self, name: &str) -> Option<Device> {
        self.host
            .input_devices()
            .ok()?
            .find(|d| d.name().ok().as_deref() == Some(name))
    }

    pub fn find_output(&self, name: &str) -> Option<Device> {
        self.host
            .output_devices()
            .ok()?
            .find(|d| d.name().ok().as_deref() == Some(name))
    }

    pub fn default_input(&self) -> Option<Device> {
        self.host.default_input_device()
    }

    pub fn default_output(&self) -> Option<Device> {
        self.host.default_output_device()
    }
}
