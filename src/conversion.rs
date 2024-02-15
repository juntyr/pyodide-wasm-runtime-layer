use std::sync::OnceLock;

use pyo3::{
    intern,
    prelude::*,
    types::{IntoPyDict, PyBool, PyDict},
};
use wasm_runtime_layer::{
    backend::{Extern, Value},
    ValueType,
};

use crate::Engine;

/// Converts a Rust type to Python
pub trait ToPy {
    /// Convert this value to Python
    fn to_py(&self, py: Python) -> Py<PyAny>;

    fn to_py_js(&self, py: Python) -> Result<Py<PyAny>, PyErr> {
        // let object = self.to_py(py);
        // let object = py_to_js(py, object.as_ref(py))?;
        // Ok(object.into_py(py))
        Ok(self.to_py(py))
    }
}

impl ToPy for Value<Engine> {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!(ty = ?self.ty(), "Value::to_py");

        match self {
            Value::I32(v) => v.to_object(py),
            Value::I64(v) => v.to_object(py),
            Value::F32(v) => v.to_object(py),
            Value::F64(v) => v.to_object(py),
            Value::FuncRef(Some(func)) => func.to_py(py),
            Value::FuncRef(None) => py.None(),
            Value::ExternRef(Some(r#ref)) => r#ref.to_py(py),
            Value::ExternRef(None) => py.None(),
        }
    }

    // fn to_py_js(&self, py: Python) -> Result<Py<PyAny>, PyErr> {
    //     #[cfg(feature = "tracing")]
    //     tracing::trace!(ty = ?self.ty(), "Value::to_py_js");

    //     if let Value::FuncRef(Some(func)) = self {
    //         let func = func.to_py(py);
    //         let func = py_to_js_proxy(py, func.as_ref(py))?;
    //         return Ok(func.into_py(py));
    //     }

    //     let object = self.to_py(py);
    //     let object = py_to_js(py, object.as_ref(py))?;
    //     Ok(object.into_py(py))
    // }
}

impl ToPy for Extern<Engine> {
    fn to_py(&self, py: Python) -> Py<PyAny> {
        #[cfg(feature = "tracing")]
        tracing::trace!("Extern::to_py");

        match self {
            Extern::Global(v) => v.to_py(py),
            Extern::Table(v) => v.to_py(py),
            Extern::Memory(v) => v.to_py(py),
            Extern::Func(v) => v.to_py(py),
        }
    }

    // fn to_py_js(&self, py: Python) -> Result<Py<PyAny>, PyErr> {
    //     #[cfg(feature = "tracing")]
    //     tracing::trace!("Extern::to_py_js");

    //     if let Extern::Func(func) = self {
    //         let func = func.to_py(py);
    //         let func = py_to_js_proxy(py, func.as_ref(py))?;
    //         return Ok(func.into_py(py));
    //     }

    //     let object = self.to_py(py);
    //     let object = py_to_js(py, object.as_ref(py))?;
    //     Ok(object.into_py(py))
    // }
}

pub trait ValueExt: Sized {
    /// Convert a value to its type
    fn ty(&self) -> ValueType;

    /// Convert the [`PyAny`] value into a Value of the supplied type
    fn from_py_typed(value: &PyAny, ty: &ValueType) -> anyhow::Result<Self>;
}

impl ValueExt for Value<Engine> {
    /// Convert a value to its type
    fn ty(&self) -> ValueType {
        match self {
            Value::I32(_) => ValueType::I32,
            Value::I64(_) => ValueType::I64,
            Value::F32(_) => ValueType::F32,
            Value::F64(_) => ValueType::F64,
            Value::FuncRef(_) => ValueType::FuncRef,
            Value::ExternRef(_) => ValueType::ExternRef,
        }
    }

    fn from_py_typed(value: &PyAny, ty: &ValueType) -> anyhow::Result<Self> {
        match ty {
            ValueType::I32 => Ok(Value::I32(value.extract()?)),
            ValueType::I64 => Ok(Value::I64(value.extract()?)),
            ValueType::F32 => Ok(Value::F32(value.extract()?)),
            ValueType::F64 => Ok(Value::F64(value.extract()?)),
            ValueType::FuncRef | ValueType::ExternRef => {
                anyhow::bail!(
                    "conversion to a function or extern outside of a module not permitted"
                )
            }
        }
    }
}

pub trait ValueTypeExt {
    /// Converts this type into the canonical ABI kind
    ///
    /// See: <https://webassembly.github.io/spec/js-api/#globals>
    fn as_js_descriptor(&self) -> &str;
}

impl ValueTypeExt for ValueType {
    fn as_js_descriptor(&self) -> &str {
        match self {
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::FuncRef => "anyfunc",
            Self::ExternRef => "externref",
        }
    }
}

/// Check if `object` is an instance of the JavaScript class with `constructor`.
pub fn instanceof(py: Python, object: &PyAny, constructor: &PyAny) -> Result<bool, PyErr> {
    fn is_instance_of(py: Python) -> &PyAny {
        static IS_INSTANCE_OF: OnceLock<Py<PyAny>> = OnceLock::new();
        // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
        IS_INSTANCE_OF.get_or_init(|| {
            py
                .import(intern!(py, "pyodide")).unwrap()
                .getattr(intern!(py, "code")).unwrap()
                .getattr(intern!(py, "run_js")).unwrap()
                .call1((
                    "function isInstanceOf(object, constructor){ return (object instanceof \
                    constructor); } isInstanceOf",
                )).unwrap()
                .into_py(py)
        }).as_ref(py)
    }

    is_instance_of(py).call1((object, constructor))?.extract()
}

// pub fn py_to_js<'py>(py: Python<'py>, object: &'py PyAny) -> Result<&'py PyAny, PyErr> {
//     py.import(intern!(py, "pyodide"))?
//         .getattr(intern!(py, "ffi"))?
//         .getattr(intern!(py, "to_js"))?
//         .call(
//             (object,),
//             Some([(intern!(py, "create_pyproxies"), false)].into_py_dict(py)),
//         )
// }

pub fn py_to_js_proxy<'py>(py: Python<'py>, object: &'py PyAny) -> Result<&'py PyAny, PyErr> {
    py.import(intern!(py, "pyodide"))?
        .getattr(intern!(py, "ffi"))?
        .getattr(intern!(py, "to_js"))?
        .call(
            (object,),
            Some([(intern!(py, "create_pyproxies"), true)].into_py_dict(py)),
        )
}

pub fn py_dict_to_js_object<'py>(py: Python<'py>, dict: &'py PyDict) -> Result<&'py PyAny, PyErr> {
    let object_from_entries = py
        .import(intern!(py, "js"))?
        .getattr(intern!(py, "Object"))?
        .getattr(intern!(py, "fromEntries"))?;

    py.import(intern!(py, "pyodide"))?
        .getattr(intern!(py, "ffi"))?
        .getattr(intern!(py, "to_js"))?
        .call(
            (dict,),
            Some(
                [
                    (intern!(py, "create_pyproxies"), &**PyBool::new(py, false)),
                    (intern!(py, "dict_converter"), object_from_entries),
                ]
                .into_py_dict(py),
            ),
        )
}

// pub fn py_to_weak_js<'py>(py: Python<'py>, object: &'py PyAny) -> Result<&'py PyAny, PyErr> {
//     fn create_weak_ref_function(py: Python) -> &PyAny {
//         static CREATE_WEAK_REF_FUNCTION: OnceLock<Py<PyAny>> = OnceLock::new();
//         // TODO: propagate error once [`OnceCell::get_or_try_init`] is stable
//         CREATE_WEAK_REF_FUNCTION.get_or_init(|| {
//             py
//                 .import(intern!(py, "pyodide")).unwrap()
//                 .getattr(intern!(py, "code")).unwrap()
//                 .getattr(intern!(py, "run_js")).unwrap()
//                 .call1((
//                     "function createWeakRefFunction(func){ let weak = new WeakRef(func); function weakRefFunction(...args) { return weak.deref()(...args); }; return weakRefFunction; } createWeakRefFunction",
//                 )).unwrap()
//                 .into_py(py)
//         }).as_ref(py)
//     }

//     // py_to_js(
//     //     py,
//         create_weak_ref_function(py).call1((/*py_to_js(py, */object/*)?*/,))//?,
//     // )
// }
