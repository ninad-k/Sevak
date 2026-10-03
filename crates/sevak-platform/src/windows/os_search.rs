//! Whole-disk search on Windows: "Everything" when it is running, otherwise
//! Windows Search.
//!
//! Windows Search is queried like a database: the `Search.CollatorDSO` OLE DB
//! provider behind ADO, with the SQL from [`windows_search_sql`]. ADO is a
//! late-bound (IDispatch) automation library with no type information in the
//! `windows` crate, so [`Object`] is a small wrapper that calls its methods by
//! name. Only five calls are needed: create the connection, set its timeout,
//! `Open`, `Execute`, and `GetString`, which returns the whole result set as
//! one string.

use std::mem::ManuallyDrop;
use std::ptr;

use windows::core::{BSTR, GUID, HSTRING, PCWSTR};
use windows::Win32::System::Com::{
    CLSIDFromProgID, CoCreateInstance, IDispatch, CLSCTX_INPROC_SERVER, DISPATCH_FLAGS,
    DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT, DISPPARAMS, EXCEPINFO,
};
use windows::Win32::System::Variant::VARIANT;

use super::com::ComGuard;
use crate::os_search::{
    everything_args, first_program, hits_from_paths, parse_path_lines, parse_windows_search_rows,
    run_lines, timed_out_or, windows_search_sql, OsHit, OsSearchError, OsSearchKind,
    OsSearchRequest,
};

/// `DISPID_PROPERTYPUT`: the named argument of a property assignment.
const DISPID_PROPERTYPUT: i32 = -3;
/// `LOCALE_USER_DEFAULT`.
const LOCALE: u32 = 0x0400;
/// ADO's `adClipString`: `GetString` returns delimited text.
const AD_CLIP_STRING: i32 = 2;
const CONNECTION_STRING: &str =
    "Provider=Search.CollatorDSO;Extended Properties='Application=Windows';";

/// Searches with Everything for names when `es.exe` is installed and Everything
/// is running, and with Windows Search otherwise.
pub(crate) fn search(
    request: &OsSearchRequest,
    words: &[String],
) -> Result<Vec<OsHit>, OsSearchError> {
    // Both sources return unordered hits, and the caller drops generated
    // folders and hidden paths, so ask for more than `limit`.
    let wanted = request.limit * 4;
    if request.kind == OsSearchKind::Names {
        if let Some(es) = first_program(&["es"]) {
            match everything(&es, request, words, wanted) {
                Ok(hits) => return Ok(hits),
                // Not running, or an unexpected answer: Windows Search can still help.
                Err(err) => tracing::debug!(%err, "Everything did not answer"),
            }
        }
    }
    windows_search(request, words, wanted)
}

fn everything(
    es: &std::path::Path,
    request: &OsSearchRequest,
    words: &[String],
    wanted: usize,
) -> Result<Vec<OsHit>, OsSearchError> {
    let output = run_lines(es, &everything_args(words, wanted), request.timeout, wanted)?;
    let paths = parse_path_lines(&output.lines);
    // `es.exe` exits with a non-zero code (8: "Everything IPC not found") and
    // prints no paths when Everything is not running.
    if paths.is_empty() && output.code.is_some_and(|code| code != 0) {
        return Err(OsSearchError::Unavailable(
            "Everything is not running".to_owned(),
        ));
    }
    let paths = timed_out_or(output.timed_out, paths)?;
    Ok(hits_from_paths(paths, wanted))
}

fn windows_search(
    request: &OsSearchRequest,
    words: &[String],
    wanted: usize,
) -> Result<Vec<OsHit>, OsSearchError> {
    let sql = windows_search_sql(words, request.kind, wanted);
    let seconds = request.timeout.as_secs_f64().ceil().max(1.0) as i32;
    let _com = ComGuard::new();
    let text = run_sql(&sql, seconds)?;
    Ok(parse_windows_search_rows(&text))
}

fn run_sql(sql: &str, timeout_seconds: i32) -> Result<String, OsSearchError> {
    const UNAVAILABLE: &str = "Windows Search is not available; start the \"Windows Search\" service (WSearch) to search the whole disk";
    let connection = Object::create("ADODB.Connection").map_err(|err| {
        tracing::debug!(%err, "ADODB is not available");
        OsSearchError::Unavailable(UNAVAILABLE.to_owned())
    })?;
    connection
        .put("CommandTimeout", VARIANT::from(timeout_seconds))
        .map_err(OsSearchError::Failed)?;
    connection
        .call("Open", &[VARIANT::from(CONNECTION_STRING)])
        .map_err(|err| {
            tracing::debug!(%err, "could not open the Windows Search index");
            OsSearchError::Unavailable(UNAVAILABLE.to_owned())
        })?;

    let result = (|| {
        let recordset = connection
            .call("Execute", &[VARIANT::from(sql)])
            .map_err(OsSearchError::Failed)?;
        let recordset = IDispatch::try_from(&recordset)
            .map(Object)
            .map_err(|err| OsSearchError::Failed(err.to_string()))?;
        // `GetString` fails on an empty result set instead of returning "".
        let at_end = recordset
            .call("EOF", &[])
            .map_err(OsSearchError::Failed)
            .map(|value| bool::try_from(&value).unwrap_or(true))?;
        if at_end {
            let _ = recordset.call("Close", &[]);
            return Ok(String::new());
        }
        let rows = recordset
            .call(
                "GetString",
                &[
                    VARIANT::from(AD_CLIP_STRING),
                    VARIANT::from(-1),
                    VARIANT::from("\t"),
                    VARIANT::from("\n"),
                    VARIANT::from(""),
                ],
            )
            .map_err(OsSearchError::Failed)?;
        let _ = recordset.call("Close", &[]);
        // An empty result set comes back as NULL rather than "".
        Ok(BSTR::try_from(&rows)
            .map(|text| text.to_string())
            .unwrap_or_default())
    })();
    let _ = connection.call("Close", &[]);
    result
}

/// An automation object called by method name.
struct Object(IDispatch);

impl Object {
    fn create(prog_id: &str) -> windows::core::Result<Self> {
        // SAFETY: the ProgID string outlives the call; the returned CLSID is
        // plain data and the object is created in-process with a ref-counted
        // interface owned by `Self`.
        unsafe {
            let clsid = CLSIDFromProgID(&HSTRING::from(prog_id))?;
            let dispatch: IDispatch = CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self(dispatch))
        }
    }

    fn call(&self, name: &str, args: &[VARIANT]) -> Result<VARIANT, String> {
        self.invoke(name, DISPATCH_METHOD | DISPATCH_PROPERTYGET, args)
    }

    fn put(&self, name: &str, value: VARIANT) -> Result<(), String> {
        self.invoke(name, DISPATCH_PROPERTYPUT, &[value]).map(drop)
    }

    fn invoke(
        &self,
        name: &str,
        flags: DISPATCH_FLAGS,
        args: &[VARIANT],
    ) -> Result<VARIANT, String> {
        let wide = HSTRING::from(name);
        let names = [PCWSTR(wide.as_ptr())];
        let mut dispid = 0_i32;
        // SAFETY: `names` holds one NUL-terminated string that outlives the
        // call, and `dispid` receives exactly one id.
        unsafe {
            self.0
                .GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, LOCALE, &mut dispid)
        }
        .map_err(|err| format!("{name}: {err}"))?;

        // IDispatch takes the arguments last to first.
        let mut reversed: Vec<VARIANT> = args.iter().rev().cloned().collect();
        let mut put_id = DISPID_PROPERTYPUT;
        let assigning = flags == DISPATCH_PROPERTYPUT;
        let params = DISPPARAMS {
            rgvarg: if reversed.is_empty() {
                ptr::null_mut()
            } else {
                reversed.as_mut_ptr()
            },
            rgdispidNamedArgs: if assigning {
                &mut put_id
            } else {
                ptr::null_mut()
            },
            cArgs: reversed.len() as u32,
            cNamedArgs: u32::from(assigning),
        };
        let mut result = VARIANT::default();
        let mut excep = EXCEPINFO::default();
        // SAFETY: `params` points at `reversed` (and `put_id`), which live
        // until after the call; the out parameters are valid, zeroed values.
        let outcome = unsafe {
            self.0.Invoke(
                dispid,
                &GUID::zeroed(),
                LOCALE,
                flags,
                &params,
                Some(&mut result),
                Some(&mut excep),
                None,
            )
        };
        // The provider's own message, when it gave one, says more than the HRESULT.
        let description = ManuallyDrop::into_inner(excep.bstrDescription).to_string();
        drop(ManuallyDrop::into_inner(excep.bstrSource));
        drop(ManuallyDrop::into_inner(excep.bstrHelpFile));
        match outcome {
            Ok(()) => Ok(result),
            Err(err) if description.is_empty() => Err(format!("{name}: {err}")),
            Err(_) => Err(format!("{name}: {description}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn request(text: &str, kind: OsSearchKind) -> OsSearchRequest {
        OsSearchRequest {
            text: text.to_owned(),
            kind,
            limit: 20,
            timeout: Duration::from_secs(10),
        }
    }

    /// Asks the real Windows Search index of this computer. Ignored by default:
    /// it needs the Windows Search service and an indexed profile. Run with
    /// `cargo test -p sevak-platform real_windows_search -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the Windows Search service"]
    fn real_windows_search_answers_names_and_contents() {
        let started = std::time::Instant::now();
        // "windows" is in many indexed paths on any installation.
        let names = crate::os_search::search(&request("windows", OsSearchKind::Names)).unwrap();
        println!(
            "names: {} hits in {} ms",
            names.len(),
            started.elapsed().as_millis()
        );
        assert!(!names.is_empty());
        assert!(names.iter().all(|hit| hit.path.is_absolute()));

        let started = std::time::Instant::now();
        let content = crate::os_search::search(&request("the", OsSearchKind::Content)).unwrap();
        println!(
            "content: {} hits in {} ms",
            content.len(),
            started.elapsed().as_millis()
        );
    }

    #[test]
    #[ignore = "needs the Windows Search service"]
    fn real_windows_search_survives_hostile_text() {
        for text in [
            "a') OR 1=1 --",
            "\"; DROP TABLE x",
            "name:* AND NOT",
            "%_[",
            "NEAR(a, b)",
        ] {
            let result = crate::os_search::search(&request(text, OsSearchKind::Names));
            println!("{text:?}: {:?}", result.as_ref().map(Vec::len));
            assert!(
                !matches!(result, Err(OsSearchError::Unavailable(_))),
                "{text:?}"
            );
        }
    }
}
