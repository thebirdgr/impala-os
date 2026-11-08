//! Physical input mapping. The future USB-CAN board driver decodes bus frames
//! into [`SteeringWheelButton`]s; this module maps them to the same
//! [`Command`]s the touchscreen sends. The frame decoding itself (IDs, bit
//! positions for the 2008 Impala's GMLAN) is deliberately *not* here yet:
//! it has to be captured from the real car.

use crate::command::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteeringWheelButton {
    VolumeUp,
    VolumeDown,
    SeekUp,
    SeekDown,
    /// "Source"/"Mode" key.
    Mode,
    Mute,
    /// Push-to-talk; reserved for voice (M3+).
    Voice,
}

/// Volume step per steering-wheel click, in percent.
pub const WHEEL_VOLUME_STEP: i8 = 4;

pub fn steering_wheel_command(button: SteeringWheelButton) -> Option<Command> {
    match button {
        SteeringWheelButton::VolumeUp => Some(Command::ChangeVolume(WHEEL_VOLUME_STEP)),
        SteeringWheelButton::VolumeDown => Some(Command::ChangeVolume(-WHEEL_VOLUME_STEP)),
        SteeringWheelButton::SeekUp => Some(Command::Next),
        SteeringWheelButton::SeekDown => Some(Command::Prev),
        SteeringWheelButton::Mode | SteeringWheelButton::Mute => Some(Command::TogglePlayPause),
        SteeringWheelButton::Voice => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_maps_to_shared_commands() {
        use SteeringWheelButton::*;
        assert_eq!(steering_wheel_command(SeekUp), Some(Command::Next));
        assert_eq!(steering_wheel_command(SeekDown), Some(Command::Prev));
        assert_eq!(steering_wheel_command(VolumeUp), Some(Command::ChangeVolume(4)));
        assert_eq!(steering_wheel_command(VolumeDown), Some(Command::ChangeVolume(-4)));
        assert_eq!(steering_wheel_command(Mode), Some(Command::TogglePlayPause));
        assert_eq!(steering_wheel_command(Voice), None);
    }
}
