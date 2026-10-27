pub mod boveda;
pub mod events;
pub mod notas;
pub mod propuestas;
pub mod trabajos;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error::StoreResult;

pub(crate) fn fmt_ts(dt: OffsetDateTime) -> StoreResult<String> {
    Ok(dt.format(&Rfc3339)?)
}

pub(crate) fn parse_ts(s: &str) -> StoreResult<OffsetDateTime> {
    Ok(OffsetDateTime::parse(s, &Rfc3339)?)
}
