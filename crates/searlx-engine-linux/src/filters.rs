//! Safe wrappers around WebKitGTK FFI APIs omitted from `webkit2gtk` 3.0's
//! safe surface (`WebKitUserContentFilterStore`, `WebKitUserContentFilter`,
//! and `webkit_user_media_permission_is_for_display_device`).

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::rc::Rc;
use std::time::Duration;

use gio::prelude::CancellableExt;
use glib::object::ObjectType;
use glib::translate::{FromGlibPtrFull, ToGlibPtr, from_glib};
use searlx_engine::{EngineError, EngineResult};
use webkit2gtk::ffi;

use crate::runtime::wait_until;

/// Compiled WebKit content-blocker filter (`WebKitUserContentFilter`).
pub(crate) struct UserContentFilter {
    ptr: NonNull<ffi::WebKitUserContentFilter>,
}

impl std::fmt::Debug for UserContentFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserContentFilter")
            .field("identifier", &self.identifier())
            .finish()
    }
}

impl Clone for UserContentFilter {
    fn clone(&self) -> Self {
        // SAFETY: `self.ptr` is a valid, non-null `WebKitUserContentFilter` with an active refcount.
        let raw = unsafe { ffi::webkit_user_content_filter_ref(self.ptr.as_ptr()) };
        Self {
            ptr: NonNull::new(raw).unwrap_or(self.ptr),
        }
    }
}

impl Drop for UserContentFilter {
    fn drop(&mut self) {
        // SAFETY: `self.ptr` is owned by this wrapper and decremented exactly once on drop.
        unsafe {
            ffi::webkit_user_content_filter_unref(self.ptr.as_ptr());
        }
    }
}

impl UserContentFilter {
    /// Identifier under which this filter was compiled in the store.
    pub(crate) fn identifier(&self) -> Option<String> {
        // SAFETY: `self.ptr` is valid; WebKit guarantees the returned string lives as long as the filter.
        let c_str = unsafe { ffi::webkit_user_content_filter_get_identifier(self.ptr.as_ptr()) };
        if c_str.is_null() {
            return None;
        }
        // SAFETY: `c_str` is non-null and NUL-terminated for the lifetime of `&self`.
        Some(
            unsafe { CStr::from_ptr(c_str) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    /// Attach this compiled filter to a `UserContentManager`.
    pub(crate) fn add_to(&self, manager: &webkit2gtk::UserContentManager) {
        // SAFETY: Both `manager` (bound to the GTK main thread) and `self.ptr` are valid non-null WebKit objects.
        unsafe {
            ffi::webkit_user_content_manager_add_filter(
                manager.to_glib_none().0,
                self.ptr.as_ptr(),
            );
        }
    }

    /// Remove this compiled filter from a `UserContentManager`.
    #[allow(dead_code)]
    pub(crate) fn remove_from(&self, manager: &webkit2gtk::UserContentManager) {
        // SAFETY: Both `manager` and `self.ptr` are valid non-null WebKit objects.
        unsafe {
            ffi::webkit_user_content_manager_remove_filter(
                manager.to_glib_none().0,
                self.ptr.as_ptr(),
            );
        }
    }
}

/// Persistent store that compiles WebKit JSON content-blocker rule lists into
/// binary bytecode filters (`WebKitUserContentFilterStore`).
#[derive(Clone, Debug)]
pub(crate) struct UserContentFilterStore {
    inner: glib::Object,
    path: PathBuf,
}

impl UserContentFilterStore {
    /// Open or create a content-filter store at `storage_path`.
    pub(crate) fn new(storage_path: &Path) -> EngineResult<Self> {
        if !gtk::is_initialized_main_thread() {
            return Err(EngineError::Backend(
                "UserContentFilterStore must be created on the GTK main thread".into(),
            ));
        }
        std::fs::create_dir_all(storage_path).map_err(|err| {
            EngineError::Backend(format!("failed to create content filter dir: {err}"))
        })?;
        let c_path = CString::new(storage_path.as_os_str().as_bytes()).map_err(|err| {
            EngineError::Backend(format!("invalid content filter path (interior NUL): {err}"))
        })?;

        // SAFETY: `c_path` is a valid NUL-terminated path string and GTK is initialized on this main thread.
        let raw = unsafe { ffi::webkit_user_content_filter_store_new(c_path.as_ptr()) };
        if raw.is_null() {
            return Err(EngineError::Backend(
                "webkit_user_content_filter_store_new returned null".into(),
            ));
        }

        // SAFETY: `raw` is a newly created GObject (`WebKitUserContentFilterStore`) with transfer-full ownership.
        let inner =
            unsafe { glib::Object::from_glib_full(raw.cast::<glib::gobject_ffi::GObject>()) };
        Ok(Self {
            inner,
            path: storage_path.to_path_buf(),
        })
    }

    /// Storage directory used by this filter store.
    #[allow(dead_code)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn as_raw(&self) -> *mut ffi::WebKitUserContentFilterStore {
        self.inner.as_ptr().cast()
    }

    /// Compile and persist `rules_json` under `identifier`, waiting up to `timeout`
    /// on the GLib main context.
    pub(crate) fn save_sync(
        &self,
        identifier: &str,
        rules_json: &str,
        timeout: Duration,
    ) -> EngineResult<UserContentFilter> {
        let c_id = CString::new(identifier).map_err(|err| {
            EngineError::Backend(format!("invalid filter identifier (interior NUL): {err}"))
        })?;
        let bytes = glib::Bytes::from(rules_json.as_bytes());
        let cancellable = gio::Cancellable::new();
        let outcome: Rc<RefCell<Option<Result<UserContentFilter, String>>>> =
            Rc::new(RefCell::new(None));
        let outcome_cb = outcome.clone();

        self.save_async(&c_id, &bytes, &cancellable, move |result| {
            *outcome_cb.borrow_mut() = Some(result);
        });

        let finished = wait_until(timeout, || outcome.borrow().is_some());
        if !finished {
            cancellable.cancel();
            return Err(EngineError::Backend(format!(
                "timed out compiling content filter {identifier:?}"
            )));
        }

        match outcome.borrow_mut().take() {
            Some(Ok(filter)) => Ok(filter),
            Some(Err(msg)) => Err(EngineError::Backend(format!(
                "content filter {identifier:?} failed to compile: {msg}"
            ))),
            None => Err(EngineError::Backend(format!(
                "content filter {identifier:?} finished without result"
            ))),
        }
    }

    fn save_async<F: FnOnce(Result<UserContentFilter, String>) + 'static>(
        &self,
        identifier: &CStr,
        source: &glib::Bytes,
        cancellable: &gio::Cancellable,
        callback: F,
    ) {
        let user_data: Box<glib::thread_guard::ThreadGuard<F>> =
            Box::new(glib::thread_guard::ThreadGuard::new(callback));

        // SAFETY: `user_data` is a `Box<ThreadGuard<F>>` transferred across the GIO async callback
        // on the same GTK main thread; `store_obj` is the `WebKitUserContentFilterStore` GObject.
        unsafe extern "C" fn save_trampoline<
            F: FnOnce(Result<UserContentFilter, String>) + 'static,
        >(
            source_object: *mut glib::gobject_ffi::GObject,
            res: *mut gio::ffi::GAsyncResult,
            user_data: glib::ffi::gpointer,
        ) {
            // SAFETY: Reconstruct owned callback box and invoke `webkit_user_content_filter_store_save_finish`.
            unsafe {
                let mut error: *mut glib::ffi::GError = ptr::null_mut();
                let filter_ptr = ffi::webkit_user_content_filter_store_save_finish(
                    source_object.cast::<ffi::WebKitUserContentFilterStore>(),
                    res,
                    &mut error,
                );
                let result = if !error.is_null() {
                    let gerr = glib::Error::from_glib_full(error);
                    Err(gerr.message().to_owned())
                } else if let Some(nn) = NonNull::new(filter_ptr) {
                    Ok(UserContentFilter { ptr: nn })
                } else {
                    Err("save_finish returned null without an error".to_owned())
                };
                let boxed: Box<glib::thread_guard::ThreadGuard<F>> =
                    Box::from_raw(user_data.cast());
                boxed.into_inner()(result);
            }
        }

        // SAFETY: All pointers (`self.as_raw()`, `identifier`, `source`, `cancellable`) remain valid for
        // the FFI call; `save_trampoline` frees `user_data` exactly once when GIO completes.
        unsafe {
            ffi::webkit_user_content_filter_store_save(
                self.as_raw(),
                identifier.as_ptr(),
                source.to_glib_none().0,
                cancellable.to_glib_none().0,
                Some(save_trampoline::<F>),
                Box::into_raw(user_data).cast(),
            );
        }
    }

    /// Remove any persisted filter with `identifier`, ignoring missing-filter errors.
    #[allow(dead_code)]
    pub(crate) fn remove_sync(&self, identifier: &str, timeout: Duration) -> EngineResult<()> {
        let c_id = CString::new(identifier).map_err(|err| {
            EngineError::Backend(format!("invalid filter identifier (interior NUL): {err}"))
        })?;
        let cancellable = gio::Cancellable::new();
        let outcome: Rc<RefCell<Option<Result<(), String>>>> = Rc::new(RefCell::new(None));
        let outcome_cb = outcome.clone();

        self.remove_async(&c_id, &cancellable, move |result| {
            *outcome_cb.borrow_mut() = Some(result);
        });

        let finished = wait_until(timeout, || outcome.borrow().is_some());
        if !finished {
            cancellable.cancel();
            return Err(EngineError::Backend(format!(
                "timed out removing content filter {identifier:?}"
            )));
        }

        match outcome.borrow_mut().take() {
            Some(Ok(())) => Ok(()),
            Some(Err(msg)) => Err(EngineError::Backend(format!(
                "failed to remove content filter {identifier:?}: {msg}"
            ))),
            None => Err(EngineError::Backend(format!(
                "remove content filter {identifier:?} finished without result"
            ))),
        }
    }

    #[allow(dead_code)]
    fn remove_async<F: FnOnce(Result<(), String>) + 'static>(
        &self,
        identifier: &CStr,
        cancellable: &gio::Cancellable,
        callback: F,
    ) {
        let user_data: Box<glib::thread_guard::ThreadGuard<F>> =
            Box::new(glib::thread_guard::ThreadGuard::new(callback));

        // SAFETY: `user_data` is a `Box<ThreadGuard<F>>` delivered on the same main thread.
        unsafe extern "C" fn remove_trampoline<F: FnOnce(Result<(), String>) + 'static>(
            source_object: *mut glib::gobject_ffi::GObject,
            res: *mut gio::ffi::GAsyncResult,
            user_data: glib::ffi::gpointer,
        ) {
            // SAFETY: Reconstruct callback box and finalize `remove_finish`.
            unsafe {
                let mut error: *mut glib::ffi::GError = ptr::null_mut();
                let ok: bool = from_glib(ffi::webkit_user_content_filter_store_remove_finish(
                    source_object.cast::<ffi::WebKitUserContentFilterStore>(),
                    res,
                    &mut error,
                ));
                let result = if !error.is_null() {
                    let gerr = glib::Error::from_glib_full(error);
                    // Removing a filter that does not exist yet is harmless.
                    if gerr.matches(webkit2gtk::UserContentFilterError::NotFound) {
                        Ok(())
                    } else {
                        Err(gerr.message().to_owned())
                    }
                } else {
                    let _ = ok;
                    Ok(())
                };
                let boxed: Box<glib::thread_guard::ThreadGuard<F>> =
                    Box::from_raw(user_data.cast());
                boxed.into_inner()(result);
            }
        }

        // SAFETY: All pointers remain valid for the FFI call; `remove_trampoline` frees `user_data`.
        unsafe {
            ffi::webkit_user_content_filter_store_remove(
                self.as_raw(),
                identifier.as_ptr(),
                cancellable.to_glib_none().0,
                Some(remove_trampoline::<F>),
                Box::into_raw(user_data).cast(),
            );
        }
    }
}

/// Check whether a `WebKitUserMediaPermissionRequest` is requesting screen/display capture.
pub(crate) fn is_display_capture_permission(req: &webkit2gtk::UserMediaPermissionRequest) -> bool {
    // SAFETY: `req` is a valid `WebKitUserMediaPermissionRequest` on the GTK main thread.
    unsafe {
        from_glib(ffi::webkit_user_media_permission_is_for_display_device(
            req.to_glib_none().0,
        ))
    }
}
