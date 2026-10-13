//! TSF 文本服务的 COM 外壳。DLL 入口在这里, 类厂在 factory, 文本服务在 service, 自注册在 registry。
#![allow(non_snake_case)] // 导出的 Dll* 入口按 COM 约定命名
#![allow(dead_code)] // M1 暂未用到 session_id, M2 启用

pub(crate) mod factory;
pub(crate) mod log;
pub(crate) mod registry;
pub(crate) mod service;

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicIsize, AtomicPtr, Ordering};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_FAIL, HINSTANCE, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::core::{BOOL, GUID, HRESULT, HSTRING, Interface};

/// 文本服务的 CLSID (自生成)。InprocServer32 / TSF profile / DllGetClassObject 都认它。
pub(crate) const CLSID_TAIYANG: GUID = GUID::from_u128(0xa7c3e5f9_1b2d_4c8e_9f0a_3d5b7e9c1f24);

/// CLSID_TAIYANG 的注册表字符串形式, 两者必须同步改。
pub(crate) const CLSID_TAIYANG_STR: &str = "{A7C3E5F9-1B2D-4C8E-9F0A-3D5B7E9C1F24}";

/// 语言 profile 的 GUID。
pub(crate) const GUID_PROFILE: GUID = GUID::from_u128(0xc9e1f2a3_4b5c_6d7e_8f90_a1b2c3d4e5f6);

/// zh-CN。
pub(crate) const LANGID_ZH_CN: u16 = 0x0804;

/// 输入法在系统里显示的名字。
pub(crate) const SERVICE_DESCRIPTION: &str = "太阳输入法";

/// 存活的 COM 对象 + LockServer 计数, DllCanUnloadNow 据它判断能否卸载。
static DLL_REFERENCES: AtomicIsize = AtomicIsize::new(0);

/// 本 DLL 的模块句柄, DllMain 加载时记下。
static DLL_MODULE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(crate) fn lock_module() {
    DLL_REFERENCES.fetch_add(1, Ordering::SeqCst);
}

pub(crate) fn unlock_module() {
    DLL_REFERENCES.fetch_sub(1, Ordering::SeqCst);
}

/// 本 DLL 在磁盘上的完整路径, 注册 InprocServer32 用。
pub(crate) fn module_path() -> windows::core::Result<HSTRING> {
    let module = HMODULE(DLL_MODULE.load(Ordering::SeqCst));
    let mut buf = [0u16; 260];
    let len = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    if len == 0 || len >= buf.len() {
        return Err(E_FAIL.into());
    }
    Ok(HSTRING::from_wide(&buf[..len]))
}

/// 本线程的会话号: 线程 id (青简经验: TSF 的 client id 各进程撞号, 线程 id 全系统唯一)。M2 启用。
pub(crate) fn session_id() -> u64 {
    u64::from(unsafe { GetCurrentThreadId() })
}

#[unsafe(no_mangle)]
extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_FAIL;
    }
    if unsafe { *rclsid } != CLSID_TAIYANG {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = factory::ClassFactory.into();
    unsafe { factory.query(riid, ppv) }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if DLL_REFERENCES.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllRegisterServer() -> HRESULT {
    match registry::register() {
        Ok(()) => S_OK,
        Err(error) => {
            log::log(&format!("DllRegisterServer 失败: {error}"));
            error.code()
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllUnregisterServer() -> HRESULT {
    match registry::unregister() {
        Ok(()) => S_OK,
        Err(error) => {
            log::log(&format!("DllUnregisterServer 失败: {error}"));
            error.code()
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(hinst: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    // 风险审查改动: 青简在 ATTACH 里调日志(文件IO) —— 加载器锁下做文件IO有死锁风险, 不抄。
    // DllMain 只存模块句柄; 日志推迟到 Activate 第一次调用时写。
    if reason == DLL_PROCESS_ATTACH {
        DLL_MODULE.store(hinst.0, Ordering::SeqCst);
    }
    true.into()
}