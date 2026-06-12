use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;
use tracing::field::{Field, Visit};

#[derive(Default)]
pub(crate) struct JsonVisitor {
    pub(crate) message: Option<String>,
    pub(crate) fields: BTreeMap<String, Value>,
}

impl JsonVisitor {
    fn record_message(&mut self, value: String) {
        if self.message.is_none() {
            self.message = Some(value);
        } else {
            self.fields
                .insert("message".to_string(), Value::String(value));
        }
    }
}

impl Visit for JsonVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.record_message(value.to_string());
        } else {
            self.fields
                .insert(field.name().to_string(), Value::String(value.to_string()));
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), Value::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), Value::Number(value.into()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), Value::Number(value.into()));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let v = serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number);
        self.fields.insert(field.name().to_string(), v);
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.fields
            .insert(field.name().to_string(), Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let s = format!("{value:?}");
        if field.name() == "message" {
            self.record_message(s);
        } else {
            self.fields
                .insert(field.name().to_string(), Value::String(s));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Subscriber;
    use tracing_subscriber::layer::SubscriberExt;

    fn captured_visitor(emit: impl FnOnce()) -> JsonVisitor {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Option<JsonVisitor>>> = Arc::new(Mutex::new(None));

        struct CaptureLayer {
            out: Arc<Mutex<Option<JsonVisitor>>>,
        }
        impl<S: Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                let mut v = JsonVisitor::default();
                event.record(&mut v);
                *self.out.lock().unwrap() = Some(v);
            }
        }

        let subscriber = tracing_subscriber::registry().with(CaptureLayer {
            out: captured.clone(),
        });
        tracing::subscriber::with_default(subscriber, emit);

        captured
            .lock()
            .unwrap()
            .take()
            .expect("an event was captured")
    }

    #[test]
    fn a_message_is_lifted_and_fields_are_typed() {
        let v = captured_visitor(|| {
            tracing::info!(count = 3_i64, ok = true, "hi");
        });
        assert_eq!(v.message.as_deref(), Some("hi"));
        assert_eq!(v.fields["count"], Value::Number(3.into()));
        assert_eq!(v.fields["ok"], Value::Bool(true));
    }

    #[test]
    fn a_non_finite_float_records_as_null_not_a_fake_zero() {
        let v = captured_visitor(|| {
            tracing::info!(ratio = f64::NAN, "m");
        });
        assert_eq!(v.fields["ratio"], Value::Null);

        let v = captured_visitor(|| {
            tracing::info!(ratio = f64::INFINITY, "m");
        });
        assert_eq!(v.fields["ratio"], Value::Null);
    }

    #[test]
    fn a_finite_float_records_as_a_number() {
        let v = captured_visitor(|| {
            tracing::info!(ratio = 1.5_f64, "m");
        });
        assert_eq!(v.fields["ratio"].as_f64(), Some(1.5));
    }

    #[test]
    fn an_explicit_message_field_does_not_clobber_the_real_message() {
        let v = captured_visitor(|| {
            tracing::info!(message = "stray", "the real message");
        });
        assert_eq!(
            v.message.as_deref(),
            Some("the real message"),
            "the format-string message wins as msg"
        );
        assert_eq!(
            v.fields.get("message"),
            Some(&Value::String("stray".to_string())),
            "the stray message field is preserved, not dropped"
        );
    }
}
