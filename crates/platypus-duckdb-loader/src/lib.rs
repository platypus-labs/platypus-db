//! Loads DuckDB's shared library (`libduckdb`) at run time, for the `duckdb` crate built as
//! for a loadable extension: that crate calls DuckDB through a table of function pointers
//! instead of linking it. [`load`] fills the table from the library's exported functions,
//! as DuckDB does for its extensions. The app downloads the library at first use
//! (`platypus-driver-duckdb`'s `library`), so the installers do not carry it.
//!
//! The only `unsafe` code of the workspace is here: loading a library runs its
//! initialisers, and a function pointer found by name is trusted to have the signature the
//! bindings give it.

use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};

use duckdb::ffi;

/// The DuckDB release the bindings were generated from; another one is refused.
pub const VERSION: &str = "v2.0.";

/// Loads the library at `path` and hands its functions to the `duckdb` crate. It stays
/// loaded for the rest of the process. Call it once: the caller keeps track.
///
/// Returns DuckDB's version, e.g. `v2.0.0`.
pub fn load(path: &Path) -> Result<String, String> {
    #[expect(unsafe_code, reason = "loading a library runs its code")]
    // SAFETY: DuckDB's initialisers have no requirements on the loading thread.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
    let api = table(&library);
    let Some(library_version) = api.duckdb_library_version else {
        return Err("it is not a DuckDB library".to_owned());
    };
    #[expect(unsafe_code, reason = "a call through the library's function")]
    // SAFETY: `duckdb_library_version` takes nothing and returns a static C string.
    let version = unsafe { CStr::from_ptr(library_version()) }
        .to_string_lossy()
        .into_owned();
    if !version.starts_with(VERSION) {
        return Err(format!(
            "it is DuckDB {version}, where PlatypusDB needs {VERSION}x"
        ));
    }
    // The functions stay in memory as long as the process.
    std::mem::forget(library);
    API.store(Box::into_raw(Box::new(api)), Ordering::Release);
    let access = ffi::duckdb_extension_access {
        set_error: None,
        get_database: None,
        get_api: Some(get_api),
    };
    #[expect(unsafe_code, reason = "hands the table to the bindings")]
    // SAFETY: `get_api` returns the table filled from the library, which stays loaded.
    let accepted = unsafe {
        ffi::duckdb_rs_extension_api_init(std::ptr::null_mut(), &raw const access, &version)
    }?;
    if accepted {
        Ok(version)
    } else {
        Err("the bindings refused DuckDB's functions".to_owned())
    }
}

/// The table [`get_api`] hands to the bindings, leaked: it lives as long as the process.
static API: AtomicPtr<ffi::duckdb_ext_api_v1> = AtomicPtr::new(std::ptr::null_mut());

#[expect(unsafe_code, reason = "the C signature of `get_api`")]
unsafe extern "C" fn get_api(
    _info: ffi::duckdb_extension_info,
    _version: *const c_char,
) -> *const c_void {
    API.load(Ordering::Acquire).cast_const().cast()
}

/// The library's address of `name`, or null when it does not export it: a function the
/// library lacks then fails with the bindings' message when called, instead of crashing.
fn symbol(library: &libloading::Library, name: &str) -> *const c_void {
    #[expect(unsafe_code, reason = "looks a symbol up")]
    // SAFETY: only the address is read here; it is called through the typed table.
    let found = unsafe { library.get::<unsafe extern "C" fn()>(name.as_bytes()) };
    found.map_or(std::ptr::null(), |f| *f as *const c_void)
}

/// Fills the table field by field, by name: a name the bindings lack does not compile, and
/// a field missing from the list fails the size check.
macro_rules! api_functions {
    ($($name:ident),* $(,)?) => {
        fn table(library: &libloading::Library) -> ffi::duckdb_ext_api_v1 {
            #[expect(unsafe_code, reason = "an all-`None` table")]
            // SAFETY: every field is an `Option` of a function pointer, for which zero is
            // `None`.
            let mut api: ffi::duckdb_ext_api_v1 = unsafe { std::mem::zeroed() };
            $(
                #[expect(unsafe_code, reason = "an address typed as the C function")]
                #[expect(clippy::missing_transmute_annotations, reason = "the field is the type")]
                // SAFETY: a function pointer and its `Option` have the size of an address,
                // null being `None`; the field's type is the exported function's signature.
                let function = unsafe {
                    std::mem::transmute::<*const c_void, _>(symbol(library, stringify!($name)))
                };
                api.$name = function;
            )*
            api
        }

        const _: () = assert!(
            size_of::<ffi::duckdb_ext_api_v1>()
                == [$(stringify!($name)),*].len() * size_of::<*const c_void>(),
            "api_functions.rs does not list every function of duckdb_ext_api_v1"
        );
    };
}

include!("api_functions.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_what_is_not_duckdb() {
        let dir = std::env::temp_dir().join("platypus-duckdb-loader-test");
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join(libloading::library_filename("not_duckdb"));
        std::fs::write(&fake, b"not a library").unwrap();
        assert!(load(&fake).is_err());
        assert!(load(&dir.join("missing.dll")).is_err());
    }
}
