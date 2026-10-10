//! The one text form of a stored message time (`StoredTime`, written only by
//! `utc_timestamp_text`), which imports and search day bounds use alike.

use chrono::{DateTime, Utc};

/// The one text form of a stored message time: UTC RFC 3339 with three
/// fractional digits and a `Z` suffix (`2015-03-12T18:04:22.000Z` for a whole
/// second). Every stored time and every string compared with one, such as a
/// search day bound, is written here, so they all sort as text in time order.
pub(crate) fn utc_timestamp_text(instant: DateTime<Utc>) -> StoredTime {
    StoredTime(instant.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// A message time in the text form `messages.timestamp` stores and every
/// list order, aggregate and search day bound compares as text. Only
/// [`utc_timestamp_text`] makes one, so a time built another way, such as
/// `2015-03-12T00:00:00Z`, which sorts after `2015-03-12T00:00:00.000Z`,
/// cannot reach a stored time or a comparison with one (#1963, #1965). It
/// binds as text, and decodes only text in its own form, so a stored time
/// read back to be compared again keeps the type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct StoredTime(String);

impl std::fmt::Display for StoredTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl sqlx::Type<sqlx::Sqlite> for StoredTime {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for StoredTime {
    /// Refuses text that is not the stored form, such as `…T00:00:00Z`, so a
    /// column written another way cannot become a `StoredTime` by being read.
    fn decode(value: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let text = <String as sqlx::Decode<'r, sqlx::Sqlite>>::decode(value)?;
        let stored = DateTime::parse_from_rfc3339(&text)
            .ok()
            .map(|instant| utc_timestamp_text(instant.with_timezone(&Utc)));
        match stored {
            Some(stored) if stored.0 == text => Ok(stored),
            _ => Err(format!("{text:?} is not a stored message time").into()),
        }
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for StoredTime {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <String as sqlx::Encode<'q, sqlx::Sqlite>>::encode_by_ref(&self.0, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// The stored form has three fractional digits and a `Z` for a whole
    /// second and for one with milliseconds, so it sorts as text in time
    /// order; `…T00:00:00Z` would sort after `…T00:00:00.000Z` (#1963).
    #[test]
    fn a_stored_time_has_milliseconds_and_a_z() {
        let at = |ms| utc_timestamp_text(Utc.timestamp_millis_opt(ms).single().unwrap());
        assert_eq!(
            at(1_426_183_462_000).to_string(),
            "2015-03-12T18:04:22.000Z"
        );
        assert_eq!(
            at(1_426_183_462_250).to_string(),
            "2015-03-12T18:04:22.250Z"
        );
    }

    /// A stored time read back decodes, and text in another form is refused,
    /// so reading a column cannot make a `StoredTime` that sorts wrong.
    #[tokio::test]
    async fn only_the_stored_form_decodes_as_a_stored_time() {
        use sqlx::Connection;
        let mut conn = sqlx::SqliteConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        let stored: StoredTime = sqlx::query_scalar("SELECT '2015-03-12T18:04:22.250Z'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(stored.to_string(), "2015-03-12T18:04:22.250Z");
        for other in [
            "2015-03-12T18:04:22Z",
            "2015-03-12T18:04:22.250+00:00",
            "yesterday",
        ] {
            let read = sqlx::query_scalar::<_, StoredTime>("SELECT $1")
                .bind(other)
                .fetch_one(&mut conn)
                .await;
            assert!(read.is_err(), "{other}");
        }
    }
}
