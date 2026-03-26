use crate::model::ConversionResult;
use crate::parser::SystemdUnit;
use crate::config::Config;
use crate::error::ConvertError;

pub fn convert(unit: &SystemdUnit, config: &Config) -> Result<ConversionResult, ConvertError> {
    todo!()
}
