use serde::{Deserialize, Deserializer};

pub fn deserialize_json_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum U64OrStr<'a> {
        U64(u64),
        Str(&'a str),
    }

    let u64_or_str = U64OrStr::deserialize(deserializer)?;
    match u64_or_str {
        U64OrStr::U64(u) => Ok(u),
        U64OrStr::Str(s) => s.parse().map_err(serde::de::Error::custom),
    }
}
