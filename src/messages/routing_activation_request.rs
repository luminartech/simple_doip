#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FurtherActionRequired {
    NoFurtherActionRequired,
    Reserved(u8),
    RoutingActivationRequiredToInitiateCentralSecurity,
    VehicleManufacturerSpecific(u8),
}

impl From<u8> for FurtherActionRequired {
    fn from(value: u8) -> Self {
        match value {
            0x00 => FurtherActionRequired::NoFurtherActionRequired,
            0x01..=0x0F => FurtherActionRequired::Reserved(value),
            0x10 => FurtherActionRequired::RoutingActivationRequiredToInitiateCentralSecurity,
            0x11..=0xFF => FurtherActionRequired::VehicleManufacturerSpecific(value),
        }
    }
}

impl From<FurtherActionRequired> for u8 {
    fn from(value: FurtherActionRequired) -> Self {
        match value {
            FurtherActionRequired::NoFurtherActionRequired => 0x00,
            FurtherActionRequired::Reserved(value) => value,
            FurtherActionRequired::RoutingActivationRequiredToInitiateCentralSecurity => 0x10,
            FurtherActionRequired::VehicleManufacturerSpecific(value) => value,
        }
    }
}
