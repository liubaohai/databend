// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::env;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Once;

use once_cell::sync::Lazy;
use opentelemetry::global;
use opentelemetry::sdk::propagation::TraceContextPropagator;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::RollingFileAppender;
use tracing_appender::rolling::Rotation;
use tracing_bunyan_formatter::BunyanFormattingLayer;
use tracing_bunyan_formatter::JsonStorageLayer;
use tracing_subscriber::fmt::Layer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::Registry;
use tracing_subscriber::EnvFilter;

/// Init tracing for unittest.
/// Write logs to file `unittest`.
pub fn init_default_ut_tracing() {
    static START: Once = Once::new();

    START.call_once(|| {
        let mut g = GLOBAL_UT_LOG_GUARD.as_ref().lock().unwrap();
        *g = Some(init_global_tracing("unittest", "_logs_unittest", "DEBUG"));
    });
}

static GLOBAL_UT_LOG_GUARD: Lazy<Arc<Mutex<Option<Vec<WorkerGuard>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

/// Init logging and tracing.
///
/// A local tracing collection(maybe for testing) can be done with a local jaeger server.
/// To report tracing data and view it:
///   docker run -d -p6831:6831/udp -p6832:6832/udp -p16686:16686 jaegertracing/all-in-one:latest
///   RUST_LOG=trace cargo test
///   open http://localhost:16686/
///
/// To adjust batch sending delay, use `OTEL_BSP_SCHEDULE_DELAY`:
/// RUST_LOG=trace OTEL_BSP_SCHEDULE_DELAY=1 cargo test
///
// TODO(xp): use DATABEND_JAEGER to assign jaeger server address.
pub fn init_global_tracing(app_name: &str, dir: &str, level: &str) -> Vec<WorkerGuard> {
    let mut guards = vec![];

    // Stdout layer.
    let (stdout_writer, stdout_guard) = tracing_appender::non_blocking(std::io::stdout());
    let stdout_logging_layer = Layer::new().with_writer(stdout_writer);
    guards.push(stdout_guard);

    // JSON log layer.
    let rolling_appender = RollingFileAppender::new(Rotation::HOURLY, dir, app_name);
    let (rolling_writer, rolling_writer_guard) = tracing_appender::non_blocking(rolling_appender);
    let file_logging_layer = BunyanFormattingLayer::new(app_name.to_string(), rolling_writer);
    guards.push(rolling_writer_guard);

    // Jaeger layer.
    global::set_text_map_propagator(TraceContextPropagator::new());
    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_service_name(app_name)
        .install_batch(opentelemetry::runtime::Tokio)
        .expect("install");
    let jaeger_layer = Some(tracing_opentelemetry::layer().with_tracer(tracer));

    // Use env RUST_LOG to initialize log if present.
    // Otherwise use the specified level.
    let directives = env::var(EnvFilter::DEFAULT_ENV).unwrap_or_else(|_x| level.to_string());
    let env_filter = EnvFilter::new(directives);
    let subscriber = Registry::default()
        .with(env_filter)
        .with(JsonStorageLayer)
        .with(stdout_logging_layer)
        .with(file_logging_layer)
        .with(jaeger_layer);
    tracing::subscriber::set_global_default(subscriber)
        .expect("error setting global tracing subscriber");

    guards
}
