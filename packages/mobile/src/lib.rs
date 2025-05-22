// #![doc = include_str!("../README.md")]
// #![doc(html_logo_url = "https://avatars.githubusercontent.com/u/79236386")]
// #![doc(html_favicon_url = "https://avatars.githubusercontent.com/u/79236386")]

// pub use dioxus_desktop::*;
// use dioxus_lib::prelude::*;
// use std::any::Any;

// /// Launch via the binding API
// pub fn launch(root: fn() -> Element) {
//     launch_cfg(root, vec![], vec![]);
// }

// pub fn launch_cfg(
//     root: fn() -> Element,
//     contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
//     platform_config: Vec<Box<dyn Any>>,
// ) {
//     dioxus_desktop::launch::launch_cfg(root, contexts, platform_config);
// }

#![doc = include_str!("../README.md")]
#![doc(html_logo_url = "https://avatars.githubusercontent.com/u/79236386")]
#![doc(html_favicon_url = "https://avatars.githubusercontent.com/u/79236386")]

pub use dioxus_desktop::*;
use dioxus_lib::prelude::*;

#[cfg(target_os = "android")]
use jni::JavaVM;
#[cfg(target_os = "android")]
use once_cell::sync::OnceCell;
use std::any::Any;
#[cfg(target_os = "android")]
use std::sync::{Mutex, PoisonError};

// / Launch via the binding API
// pub fn launch(root: fn() -> Element) {
//     launch_cfg(root, vec![], vec![]);
// }

// pub fn launch_cfg(
//     root: fn() -> Element,
//     contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
//     platform_config: Vec<Box<dyn Any>>,
// ) {
//     dioxus_desktop::launch::launch_cfg(root, contexts, platform_config);
// }

#[cfg(target_os = "android")]
#[no_mangle]
#[inline(never)]
pub extern "C" fn start_app() {
    tao::android_binding!(
        dev_dioxus,
        main,
        WryActivity,
        wry::android_setup,
        // dioxus_main_root_fn,
        root,
        tao
    );
    wry::android_binding!(dev_dioxus, main, wry);
}

#[cfg(target_os = "android")]
fn load_env_file_from_session_cache() {
    let env_file = dioxus_cli_config::android_session_cache_dir().join(".env");
    if let Some(env_file) = std::fs::read_to_string(&env_file).ok() {
        for line in env_file.lines() {
            if let Some((key, value)) = line.trim().split_once('=') {
                std::env::set_var(key, value);
            }
        }
    }
}

/// Launch via the binding API
pub fn launch(root: fn() -> Element) {
    launch_cfg(root, vec![], vec![]);
}

pub fn launch_cfg(
    root: fn() -> Element,
    contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
    platform_config: Vec<Box<dyn Any>>,
) {
    #[cfg(target_os = "android")]
    {
        *APP_OBJECTS.lock().unwrap() = Some(BoundLaunchObjects {
            root,
            contexts,
            platform_config,
        });
    }

    #[cfg(not(target_os = "android"))]
    {
        dioxus_desktop::launch::launch_cfg(root, contexts, platform_config);
    }
}

/// We need to store the root function and contexts in a static so that when the tao bindings call
/// "start_app", that the original function arguments are still around.
///
/// If you look closely, you'll notice that we impl Send for this struct. This would normally be
/// unsound. However, we know that the thread that created these objects ("main()" - see JNI_OnLoad)
/// is finished once `start_app` is called. This is similar to how an Rc<T> is technically safe
/// to move between threads if you can prove that no other thread is using the Rc<T> at the same time.
/// Crates like https://crates.io/crates/sendable exist that build on this idea but with runtimk,
/// validation that the current thread is the one that created the object.
///
/// Since `main()` completes, the only reader of this data will be `start_app`, so it's okay to
/// impl this as Send/Sync.
///
/// Todo(jon): the visibility of functions in this module is too public. Make sure to hide them before
/// releasing 0.7.
#[cfg(target_os = "android")]
struct BoundLaunchObjects {
    root: fn() -> Element,
    contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
    platform_config: Vec<Box<dyn Any>>,
}
#[cfg(target_os = "android")]
unsafe impl Send for BoundLaunchObjects {}
#[cfg(target_os = "android")]
unsafe impl Sync for BoundLaunchObjects {}
#[cfg(target_os = "android")]
static APP_OBJECTS: Mutex<Option<BoundLaunchObjects>> = Mutex::new(None);

#[cfg(target_os = "android")]
static JVM: OnceCell<JavaVM> = OnceCell::new();

#[cfg(target_os = "android")]
pub fn get_java_vm() -> Option<&'static JavaVM> {
    JVM.get()
}

#[cfg(target_os = "android")]
#[doc(hidden)]
pub fn root() {
    let app = APP_OBJECTS
        .lock()
        .expect("APP_FN_PTR lock failed")
        .take()
        .expect("Android to have set the app trampoline");

    let BoundLaunchObjects {
        root,
        contexts,
        platform_config,
    } = app;

    dioxus_desktop::launch::launch_cfg(root, contexts, platform_config);
}

#[cfg(target_os = "android")]
#[no_mangle]
#[inline(never)]
pub extern "C" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut libc::c_void,
) -> jni::sys::jint {
    // Capture the JavaVM instance. This MUST happen before any other JNI calls
    // or any code that might panic.
    // SAFETY: The vm pointer is guaranteed valid by the JNI spec during JNI_OnLoad.
    let java_vm = match unsafe { JavaVM::from_raw(vm) } {
        Ok(vm) => vm,
        Err(e) => {
            // Use android_log or similar if available, otherwise eprintln
            eprintln!("Failed to create JavaVM from raw pointer: {:?}", e);
            // Return JNI_ERR to indicate failure
            return jni::sys::JNI_ERR;
        }
    };

    // Store the JavaVM globally. If this fails, something is seriously wrong.
    if JVM.set(java_vm).is_err() {
        eprintln!("Failed to store JavaVM globally. Was JNI_OnLoad called twice?");
        return jni::sys::JNI_ERR;
    }

    // we're going to find the `main` symbol using dlsym directly and call it
    unsafe {
        let mut main_fn_ptr = libc::dlsym(libc::RTLD_DEFAULT, b"main\0".as_ptr() as _);

        if main_fn_ptr.is_null() {
            main_fn_ptr = libc::dlsym(libc::RTLD_DEFAULT, b"_main\0".as_ptr() as _);
        }

        if main_fn_ptr.is_null() {
            panic!("Failed to find main symbol");
        }

        // Set the env vars that rust code might expect, passed off to us by the android app
        // Doing this before main emulates the behavior of a regular executable
        if cfg!(target_os = "android") && cfg!(debug_assertions) {
            load_env_file_from_session_cache();
        }

        let main_fn: extern "C" fn() = std::mem::transmute(main_fn_ptr);
        main_fn();
    };

    jni::sys::JNI_VERSION_1_6
}

// Call our `main` function to initialize the rust runtime and set the launch binding trampoline
// #[cfg(target_os = "android")]
// #[no_mangle]
// #[inline(never)]
// pub extern "C" fn JNI_OnLoad(
//     vm: *mut jni::sys::JavaVM, // Changed type from c_void
//     _reserved: *mut libc::c_void,
// ) -> jni::sys::jint {
//     // Capture the JavaVM instance. This MUST happen before any other JNI calls
//     // or any code that might panic.
//     // SAFETY: The vm pointer is guaranteed valid by the JNI spec during JNI_OnLoad.
//     let java_vm = match unsafe { JavaVM::from_raw(vm) } {
//         Ok(vm) => vm,
//         Err(e) => {
//             // Use android_log or similar if available, otherwise eprintln
//             eprintln!("Failed to create JavaVM from raw pointer: {:?}", e);
//             // Return JNI_ERR to indicate failure
//             return jni::sys::JNI_ERR;
//         }
//     };

//     // Store the JavaVM globally. If this fails, something is seriously wrong.
//     if JVM.set(java_vm).is_err() {
//         eprintln!("Failed to store JavaVM globally. Was JNI_OnLoad called twice?");
//         return jni::sys::JNI_ERR;
//     }

//     jni::sys::JNI_VERSION_1_6
// }
