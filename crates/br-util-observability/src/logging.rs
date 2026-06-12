use std::fmt;

use serde_json::{Value, json};
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

use crate::visitor::JsonVisitor;

const RESERVED_KEYS: [&str; 4] = ["ts", "level", "component", "msg"];

pub fn init_logging(component: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .event_format(JsonEventFormatter::new(component))
        .fmt_fields(NoopFields)
        .with_writer(std::io::stdout)
        .finish();

    if tracing::subscriber::set_global_default(subscriber).is_err() {
        eprintln!("{component}: tracing subscriber already initialised, continuing");
    }
}

struct JsonEventFormatter {
    component: String,
}

impl JsonEventFormatter {
    fn new(component: &str) -> Self {
        Self {
            component: component.to_string(),
        }
    }
}

fn render_line(component: &str, level: &str, visitor: JsonVisitor) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("ts".into(), Value::String(now_rfc3339()));
    map.insert("level".into(), Value::String(level.to_string()));
    map.insert("component".into(), Value::String(component.to_string()));
    if let Some(msg) = visitor.message {
        map.insert("msg".into(), Value::String(msg));
    }
    for (k, v) in visitor.fields {
        if RESERVED_KEYS.contains(&k.as_str()) {
            continue;
        }
        map.insert(k, v);
    }
    Value::Object(map)
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}

fn level_code(level: &tracing::Level) -> &'static str {
    match *level {
        tracing::Level::ERROR => "ERROR",
        tracing::Level::WARN => "WARN",
        tracing::Level::INFO => "INFO",
        tracing::Level::DEBUG => "DEBUG",
        tracing::Level::TRACE => "TRACE",
    }
}

impl<S, N> FormatEvent<S, N> for JsonEventFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let level = level_code(event.metadata().level());

        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);

        let value = render_line(&self.component, level, visitor);
        let line = serde_json::to_string(&value).unwrap_or_else(|_| {
            json!({
                "ts": now_rfc3339(),
                "level": "ERROR",
                "component": self.component,
                "msg": "log serialisation failed",
            })
            .to_string()
        });
        writer.write_str(&line)?;
        writer.write_char('\n')
    }
}

struct NoopFields;

impl<'writer> FormatFields<'writer> for NoopFields {
    fn format_fields<R: RecordFields>(&self, _writer: Writer<'writer>, _fields: R) -> fmt::Result {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn visitor_with(msg: &str, fields: &[(&str, &str)]) -> JsonVisitor {
        let mut v = JsonVisitor {
            message: Some(msg.to_string()),
            ..Default::default()
        };
        for (k, val) in fields {
            v.fields
                .insert((*k).to_string(), Value::String((*val).to_string()));
        }
        v
    }

    #[test]
    fn render_line_emits_the_canonical_keys() {
        let line = render_line("composer", "INFO", visitor_with("hello", &[]));
        let obj = line.as_object().expect("an object");

        assert_eq!(obj["level"], json!("INFO"));
        assert_eq!(obj["component"], json!("composer"));
        assert_eq!(obj["msg"], json!("hello"));
        assert!(obj.contains_key("ts"), "ts is always present");
        assert!(obj["ts"].as_str().is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn ts_is_fixed_width_microsecond_utc() {
        let line = render_line("composer", "INFO", visitor_with("hello", &[]));
        let ts = line["ts"].as_str().expect("ts is a string");
        assert!(ts.ends_with("+00:00"), "UTC offset, got {ts:?}");
        let frac = ts
            .rsplit_once('.')
            .map(|(_, rest)| rest.trim_end_matches("+00:00"))
            .expect("a fractional part");
        assert_eq!(
            frac.len(),
            6,
            "exactly 6 (micro) fractional digits, got {ts:?}"
        );
        assert!(frac.chars().all(|c| c.is_ascii_digit()), "digits only");
        chrono::DateTime::parse_from_rfc3339(ts).expect("valid rfc3339");
    }

    #[test]
    fn render_line_serialises_to_valid_json() {
        let line = render_line("svc-notifier", "WARN", visitor_with("careful", &[]));
        let s = serde_json::to_string(&line).expect("serialisable");
        let back: Value = serde_json::from_str(&s).expect("valid json");
        assert_eq!(back["component"], json!("svc-notifier"));
        assert_eq!(back["msg"], json!("careful"));
    }

    #[test]
    fn init_logging_twice_does_not_panic() {
        init_logging("test-a");
        init_logging("test-b");
    }

    #[test]
    fn user_fields_are_carried_alongside_canonical_ones() {
        let line = render_line(
            "composer",
            "INFO",
            visitor_with("did a thing", &[("request_id", "abc-123")]),
        );
        assert_eq!(line["request_id"], json!("abc-123"));
        assert_eq!(line["msg"], json!("did a thing"));
    }

    #[test]
    fn user_fields_cannot_clobber_canonical_keys() {
        let line = render_line(
            "composer",
            "INFO",
            visitor_with("real msg", &[("level", "spoofed"), ("component", "evil")]),
        );
        assert_eq!(line["level"], json!("INFO"), "level not spoofable");
        assert_eq!(
            line["component"],
            json!("composer"),
            "component not spoofable"
        );
        assert_eq!(line["msg"], json!("real msg"));
    }

    #[test]
    fn message_is_rendered_as_msg_with_no_stray_message_key() {
        let line = render_line("c", "INFO", visitor_with("the message", &[]));
        assert_eq!(line["msg"], json!("the message"));
        assert!(line.get("message").is_none(), "no stray `message` key");
    }

    #[test]
    fn an_absent_message_omits_msg_entirely() {
        let mut v = JsonVisitor::default();
        v.fields
            .insert("k".to_string(), Value::String("v".to_string()));
        let line = render_line("c", "INFO", v);
        assert!(line.get("msg").is_none(), "no msg when there is no message");
        assert_eq!(line["k"], json!("v"));
    }

    #[test]
    fn level_code_is_the_stable_uppercase_code() {
        assert_eq!(level_code(&tracing::Level::ERROR), "ERROR");
        assert_eq!(level_code(&tracing::Level::WARN), "WARN");
        assert_eq!(level_code(&tracing::Level::INFO), "INFO");
        assert_eq!(level_code(&tracing::Level::DEBUG), "DEBUG");
        assert_eq!(level_code(&tracing::Level::TRACE), "TRACE");
    }
}
