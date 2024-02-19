use pyo3::{intern, prelude::*, types::PyDict};
use wasm_runtime_layer::{
    backend::{AsContext, AsContextMut, Value, WasmTable},
    TableType, ValueType,
};

use crate::{
    conversion::{instanceof, py_dict_to_js_object, ToPy, ValueExt, ValueTypeExt},
    Engine,
};

#[derive(Clone, Debug)]
/// A WebAssembly table
pub struct Table {
    /// Table reference
    table: Py<PyAny>,
    /// The table signature
    ty: TableType,
}

impl WasmTable<Engine> for Table {
    fn new(
        _ctx: impl AsContextMut<Engine>,
        ty: TableType,
        init: Value<Engine>,
    ) -> anyhow::Result<Self> {
        Python::with_gil(|py| -> anyhow::Result<Self> {
            #[cfg(feature = "tracing")]
            tracing::debug!(?ty, ?init, "Table::new");

            let desc = PyDict::new(py);
            desc.set_item(intern!(py, "element"), ty.element().as_js_descriptor())?;
            desc.set_item(intern!(py, "initial"), ty.minimum())?;
            if let Some(max) = ty.maximum() {
                desc.set_item(intern!(py, "maximum"), max)?;
            }
            let desc = py_dict_to_js_object(py, desc)?;

            let init = init.to_py(py);

            let table = web_assembly_table(py)?
                .getattr(intern!(py, "new"))?
                .call1((desc, init))?;

            Ok(Self {
                ty,
                table: table.into_py(py),
            })
        })
    }

    /// Returns the type and limits of the table.
    fn ty(&self, _ctx: impl AsContext<Engine>) -> TableType {
        self.ty
    }

    /// Returns the current size of the table.
    fn size(&self, _ctx: impl AsContext<Engine>) -> u32 {
        Python::with_gil(|py| -> Result<u32, PyErr> {
            let table = self.table.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%table, ?self.ty, "Table::size");

            table.getattr(intern!(py, "length"))?.extract()
        })
        .unwrap()
    }

    /// Grows the table by the given amount of elements.
    fn grow(
        &self,
        _ctx: impl AsContextMut<Engine>,
        delta: u32,
        init: Value<Engine>,
    ) -> anyhow::Result<u32> {
        Python::with_gil(|py| {
            let table = self.table.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%table, ?self.ty, delta, ?init, "Table::grow");

            let init = init.to_py(py);

            let old_len = table
                .call_method1(intern!(py, "grow"), (delta, init))?
                .extract()?;

            Ok(old_len)
        })
    }

    /// Returns the table element value at `index`.
    fn get(&self, _ctx: impl AsContextMut<Engine>, index: u32) -> Option<Value<Engine>> {
        Python::with_gil(|py| {
            let table = self.table.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%table, ?self.ty, index, "Table::get");

            let value = table.call_method1(intern!(py, "get"), (index,)).ok()?;

            Some(Value::from_py_typed(value, self.ty.element()).unwrap())
        })
    }

    /// Sets the value of this table at `index`.
    fn set(
        &self,
        _ctx: impl AsContextMut<Engine>,
        index: u32,
        value: Value<Engine>,
    ) -> anyhow::Result<()> {
        Python::with_gil(|py| {
            let table = self.table.as_ref(py);

            #[cfg(feature = "tracing")]
            tracing::debug!(%table, ?self.ty, index, ?value, "Table::set");

            let value = value.to_py(py);

            table.call_method1(intern!(py, "set"), (index, value))?;

            Ok(())
        })
    }
}

impl ToPy for Table {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(table = %self.table, ?self.ty, "Table::to_py");

        self.table.clone_ref(py)
    }
}

impl Table {
    /// Creates a new table from a Python value
    pub(crate) fn from_exported_table(
        py: Python,
        value: Py<PyAny>,
        ty: TableType,
    ) -> anyhow::Result<Self> {
        if !instanceof(py, value.as_ref(py), web_assembly_table(py)?)? {
            anyhow::bail!("expected WebAssembly.Table but found {value:?}");
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(value = %value.as_ref(py), ?ty, "Table::from_exported_table");

        let table_length: u32 = value.as_ref(py).getattr(intern!(py, "length"))?.extract()?;

        assert!(table_length >= ty.minimum());
        assert_eq!(ty.element(), ValueType::FuncRef);

        Ok(Self { ty, table: value })
    }
}

fn web_assembly_table(py: Python) -> Result<&PyAny, PyErr> {
    py.import(intern!(py, "js"))?
        .getattr(intern!(py, "WebAssembly"))?
        .getattr(intern!(py, "Table"))
}
