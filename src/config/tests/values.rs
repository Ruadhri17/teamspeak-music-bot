use serde::{Deserialize, Serialize};

use crate::config::{Bitrate, Complexity, Volume};

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct Field<T> {
    value: T,
}

fn parse<T: for<'de> Deserialize<'de>>(toml: &str) -> Result<T, String> {
    toml::from_str::<Field<T>>(toml)
        .map(|field| field.value)
        .map_err(|err| err.to_string())
}

#[test]
fn bitrate_accepts_range_bounds() {
    assert_eq!(Bitrate::new(Bitrate::MIN).map(Bitrate::get), Some(32));
    assert_eq!(Bitrate::new(96).map(Bitrate::get), Some(96));
    assert_eq!(Bitrate::new(Bitrate::MAX).map(Bitrate::get), Some(192));
}

#[test]
fn bitrate_rejects_values_outside_range() {
    for kbps in [0, 31, 193, u32::MAX] {
        assert_eq!(Bitrate::new(kbps), None, "{kbps}");
    }
}

#[test]
fn bitrate_error_message() {
    assert_eq!(
        Bitrate::try_from(500).unwrap_err(),
        "bitrate must be between 32 and 192 kb/s"
    );
}

#[test]
fn bitrate_converts_back_to_number() {
    assert_eq!(u32::from(Bitrate::new(96).unwrap()), 96);
}

#[test]
fn complexity_accepts_range_bounds() {
    assert_eq!(Complexity::new(0).map(Complexity::get), Some(0));
    assert_eq!(
        Complexity::new(Complexity::MAX).map(Complexity::get),
        Some(10)
    );
}

#[test]
fn complexity_rejects_values_outside_range() {
    for level in [11, u8::MAX] {
        assert_eq!(Complexity::new(level), None, "{level}");
    }
}

#[test]
fn complexity_error_message() {
    assert_eq!(
        Complexity::try_from(11).unwrap_err(),
        "complexity must be between 0 and 10"
    );
}

#[test]
fn complexity_converts_back_to_number() {
    assert_eq!(u8::from(Complexity::new(5).unwrap()), 5);
}

#[test]
fn volume_accepts_range_bounds() {
    assert_eq!(Volume::new(0).map(Volume::get), Some(0));
    assert_eq!(Volume::new(Volume::MAX).map(Volume::get), Some(100));
}

#[test]
fn volume_rejects_values_outside_range() {
    for percent in [101, u8::MAX] {
        assert_eq!(Volume::new(percent), None, "{percent}");
    }
}

#[test]
fn volume_error_message() {
    assert_eq!(
        Volume::try_from(150).unwrap_err(),
        "volume must be between 0 and 100"
    );
}

#[test]
fn volume_converts_back_to_number() {
    assert_eq!(u8::from(Volume::new(50).unwrap()), 50);
}

#[test]
fn values_parse_from_toml() {
    assert_eq!(
        parse::<Bitrate>("value = 96"),
        Ok(Bitrate::new(96).unwrap())
    );
    assert_eq!(
        parse::<Complexity>("value = 10"),
        Ok(Complexity::new(10).unwrap())
    );
    assert_eq!(parse::<Volume>("value = 0"), Ok(Volume::new(0).unwrap()));
}

#[test]
fn values_report_range_error_in_toml() {
    let err = parse::<Bitrate>("value = 500").unwrap_err();
    assert!(
        err.contains("bitrate must be between 32 and 192 kb/s"),
        "{err}"
    );
    let err = parse::<Complexity>("value = 11").unwrap_err();
    assert!(err.contains("complexity must be between 0 and 10"), "{err}");
    let err = parse::<Volume>("value = 101").unwrap_err();
    assert!(err.contains("volume must be between 0 and 100"), "{err}");
}

#[test]
fn values_reject_negative_numbers_and_wrong_types() {
    assert!(parse::<Volume>("value = -1").is_err());
    assert!(
        parse::<Volume>("value = 300").is_err(),
        "does not fit in u8"
    );
    assert!(parse::<Volume>("value = \"50\"").is_err());
    assert!(parse::<Bitrate>("value = 96.5").is_err());
}

#[test]
fn values_are_written_as_plain_numbers() {
    let field = Field {
        value: Volume::new(50).unwrap(),
    };
    assert_eq!(toml::to_string(&field).unwrap(), "value = 50\n");
}
