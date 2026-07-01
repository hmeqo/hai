use std::fmt;

use serde::Deserialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Lenient deserialization for Vec<i64> — accepts array, single number, or "x,y" string
// ---------------------------------------------------------------------------

struct LenientI64Visitor;

impl<'de> serde::de::Visitor<'de> for LenientI64Visitor {
    type Value = Vec<i64>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an array of integers, a single integer, or a comma-separated string")
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(vec![v])
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(vec![v as i64])
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(vec![v as i64])
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        v.split(',')
            .map(|s| s.trim().parse::<i64>().map_err(serde::de::Error::custom))
            .collect()
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut vec = Vec::new();
        while let Some(elem) = seq.next_element::<i64>()? {
            vec.push(elem);
        }
        Ok(vec)
    }
}

pub fn deserialize_lenient_i64_vec<'de, D>(d: D) -> Result<Vec<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    d.deserialize_any(LenientI64Visitor)
}

pub fn deserialize_option_lenient_i64_vec<'de, D>(d: D) -> Result<Option<Vec<i64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptVisitor;

    impl<'de> serde::de::Visitor<'de> for OptVisitor {
        type Value = Option<Vec<i64>>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an array of integers, a single integer, a comma-separated string, or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D2>(self, d2: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::de::Deserializer<'de>,
        {
            d2.deserialize_any(LenientI64Visitor).map(Some)
        }
    }

    d.deserialize_option(OptVisitor)
}

/// Lenient deserialization for `Option<u64>` — accepts number, string, or null.
pub fn deserialize_option_lenient_u64<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("expected u64"))
            .map(Some),
        Value::String(s) if s.is_empty() => Ok(None),
        Value::String(s) => s.parse::<u64>().map(Some).map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom(
            "expected a number or string representing u64",
        )),
    }
}
