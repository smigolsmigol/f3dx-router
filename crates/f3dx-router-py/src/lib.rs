//! PyO3 bridge for f3dx-router.
//!
//! Python surface:
//!   f3dx_router.Router(providers, policy='sequential', hedge_k=2)
//!   router.chat_completions(body) -> dict
//!
//! Providers come in as a list of dicts with keys:
//!   {"name": str, "kind": "openai" | "anthropic", "base_url": str,
//!    "api_key": str, "timeout_ms": int (default 30000),
//!    "weight": int (default 1)}

use f3dx_router_core::{Provider, ProviderKind, Router as CoreRouter, RouterConfig, RoutingPolicy};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::sync::Arc;
use tokio::runtime::Runtime;

#[pyclass(name = "Router", module = "f3dx_router")]
struct PyRouter {
    inner: CoreRouter,
    runtime: Arc<Runtime>,
}

fn parse_kind(s: &str) -> PyResult<ProviderKind> {
    match s.to_ascii_lowercase().as_str() {
        "openai" => Ok(ProviderKind::OpenAI),
        "anthropic" => Ok(ProviderKind::Anthropic),
        other => Err(PyValueError::new_err(format!(
            "unknown provider kind {other:?}; expected openai|anthropic"
        ))),
    }
}

fn parse_policy(s: &str) -> PyResult<RoutingPolicy> {
    match s.to_ascii_lowercase().as_str() {
        "sequential" => Ok(RoutingPolicy::Sequential),
        "hedged" => Ok(RoutingPolicy::Hedged),
        other => Err(PyValueError::new_err(format!(
            "unknown policy {other:?}; expected sequential|hedged"
        ))),
    }
}

fn parse_providers(providers: &Bound<'_, PyList>) -> PyResult<Vec<Provider>> {
    let mut out = Vec::with_capacity(providers.len());
    for item in providers.iter() {
        let d: &Bound<'_, PyDict> = item.cast()?;
        let name: String = d
            .get_item("name")?
            .ok_or_else(|| PyValueError::new_err("provider missing 'name'"))?
            .extract()?;
        let kind_str: String = d
            .get_item("kind")?
            .ok_or_else(|| PyValueError::new_err("provider missing 'kind'"))?
            .extract()?;
        let base_url: String = d
            .get_item("base_url")?
            .ok_or_else(|| PyValueError::new_err("provider missing 'base_url'"))?
            .extract()?;
        let api_key: String = d
            .get_item("api_key")?
            .ok_or_else(|| PyValueError::new_err("provider missing 'api_key'"))?
            .extract()?;
        let timeout_ms: u64 = d
            .get_item("timeout_ms")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(30_000);
        let weight: u32 = d
            .get_item("weight")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(1);
        out.push(Provider {
            name,
            kind: parse_kind(&kind_str)?,
            base_url,
            api_key,
            timeout_ms,
            weight,
        });
    }
    Ok(out)
}

fn json_value_to_py<'py>(
    py: Python<'py>,
    v: &serde_json::Value,
) -> PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
    use pyo3::IntoPyObject;
    use pyo3::types::{PyList, PyNone};
    match v {
        serde_json::Value::Null => Ok(PyNone::get(py).to_owned().into_any()),
        serde_json::Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any())
            } else {
                Ok(n.to_string().into_pyobject(py)?.into_any())
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.into_any()),
        serde_json::Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_value_to_py(py, item)?)?;
            }
            Ok(list.into_any())
        }
        serde_json::Value::Object(map) => {
            let d = PyDict::new(py);
            for (k, val) in map {
                d.set_item(k, json_value_to_py(py, val)?)?;
            }
            Ok(d.into_any())
        }
    }
}

#[pymethods]
impl PyRouter {
    #[new]
    #[pyo3(signature = (providers, policy = "sequential".to_string(), hedge_k = 2))]
    fn new(providers: Bound<'_, PyList>, policy: String, hedge_k: usize) -> PyResult<Self> {
        let providers = parse_providers(&providers)?;
        let policy = parse_policy(&policy)?;
        let config = RouterConfig {
            providers,
            policy,
            hedge_k,
        };
        let inner = CoreRouter::new(config).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let runtime =
            Runtime::new().map_err(|e| PyRuntimeError::new_err(format!("tokio runtime: {e}")))?;
        Ok(Self {
            inner,
            runtime: Arc::new(runtime),
        })
    }

    fn chat_completions<'py>(
        &self,
        py: Python<'py>,
        body_json: &str,
    ) -> PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let body: serde_json::Value =
            serde_json::from_str(body_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let inner = &self.inner;
        let runtime = Arc::clone(&self.runtime);
        let response = py
            .detach(|| runtime.block_on(async move { inner.chat_completions(body).await }));
        match response {
            Ok(value) => json_value_to_py(py, &value),
            Err(e) => Err(PyRuntimeError::new_err(e.to_string())),
        }
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRouter>()?;
    Ok(())
}
