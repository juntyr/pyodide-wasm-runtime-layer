use std::collections::BTreeMap;

use fxhash::FxHashMap;
use pyo3::{intern, prelude::*, types::IntoPyDict};
use wasm_runtime_layer::{
    backend::{AsContext, Export, Extern, Imports, WasmInstance},
    ExternType,
};

use crate::{conversion::ToPy, module::ParsedModule, Engine, Func, Global, Memory, Module, Table};

/// A WebAssembly Instance.
#[derive(Debug, Clone)]
pub struct Instance {
    /// The inner instance
    _instance: Py<PyAny>,
    /// The exports of the instance
    exports: FxHashMap<String, Extern<Engine>>,
}

impl WasmInstance<Engine> for Instance {
    fn new(
        _store: impl super::AsContextMut<Engine>,
        module: &Module,
        imports: &Imports<Engine>,
    ) -> anyhow::Result<Self> {
        Python::with_gil(|py| {
            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("Instance::new").entered();

            let imports_object = create_imports_object(py, imports);

            let instance = web_assembly_instance(py)?
                .getattr(intern!(py, "new"))?
                .call1((module.module(py), imports_object))?;

            #[cfg(feature = "tracing")]
            let _span = tracing::debug_span!("get_exports").entered();

            let exports = instance.getattr(intern!(py, "exports"))?;
            let exports = process_exports(exports, module.parsed())?;

            Ok(Self {
                _instance: instance.into_py(py),
                exports,
            })
        })
    }

    fn exports(&self, _store: impl AsContext<Engine>) -> Box<dyn Iterator<Item = Export<Engine>>> {
        Box::new(
            self.exports
                .iter()
                .map(|(name, value)| Export {
                    name: name.into(),
                    value: value.clone(),
                })
                .collect::<Vec<_>>()
                .into_iter(),
        )
    }

    fn get_export(&self, _store: impl AsContext<Engine>, name: &str) -> Option<Extern<Engine>> {
        self.exports.get(name).cloned()
    }
}

/// Creates the js import map
fn create_imports_object<'py>(py: Python<'py>, imports: &Imports<Engine>) -> &'py PyAny {
    #[cfg(feature = "tracing")]
    let _span = tracing::debug_span!("process_imports").entered();

    imports
        .into_iter()
        .map(|((module, name), import)| {
            #[cfg(feature = "tracing")]
            tracing::trace!(?module, ?name, ?import, "import");
            let import = import.to_py(py);

            #[cfg(feature = "tracing")]
            tracing::trace!(module, name, "export");

            (module, (name, import))
        })
        .fold(BTreeMap::<String, Vec<_>>::new(), |mut acc, (m, value)| {
            acc.entry(m).or_default().push(value);
            acc
        })
        .into_iter()
        .map(|(module, imports)| (module, imports.into_py_dict(py)))
        .into_py_dict(py)
        .as_ref()
}

/// Processes a wasm module's exports into a hashmap
fn process_exports(
    exports: &PyAny,
    parsed: &ParsedModule,
) -> anyhow::Result<FxHashMap<String, Extern<Engine>>> {
    let py = exports.py();

    #[cfg(feature = "tracing")]
    let _span = tracing::debug_span!("process_exports", ?exports).entered();

    exports
        .call_method0(intern!(py, "object_entries"))?
        .iter()?
        .map(|entry| {
            let (name, value): (String, &PyAny) = entry?.extract()?;

            #[cfg(feature = "tracing")]
            let _span = tracing::trace_span!("process_export", ?name, ?value).entered();

            let signature = parsed.exports.get(&name).expect("export signature").clone();

            let export = match signature {
                ExternType::Func(signature) => {
                    Extern::Func(Func::from_exported_function(value, signature)?)
                }
                ExternType::Global(signature) => {
                    Extern::Global(Global::from_exported_global(value, signature)?)
                }
                ExternType::Memory(ty) => Extern::Memory(Memory::from_exported_memory(value, ty)?),
                ExternType::Table(ty) => Extern::Table(Table::from_exported_table(value, ty)?),
            };

            Ok((name, export))
        })
        .collect()
}

fn web_assembly_instance(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Instance"))
}
