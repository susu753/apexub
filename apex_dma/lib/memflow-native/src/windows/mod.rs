use memflow::os::process::*;
use memflow::prelude::v1::*;

use windows::core::PCSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};

use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueA, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
};

use core::mem::{size_of, MaybeUninit};
use core::ptr;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

pub mod mem;
use mem::ProcessVirtualMemory;

pub mod process;
use process::WindowsProcess;

pub mod keyboard;
use keyboard::WindowsKeyboard;

struct KernelModule {}

pub(crate) struct Handle(HANDLE);

impl From<HANDLE> for Handle {
    fn from(handle: HANDLE) -> Handle {
        Handle(handle)
    }
}

impl core::ops::Deref for Handle {
    type Target = HANDLE;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) }.ok();
    }
}

pub fn conv_err(_err: windows::core::Error) -> Error {
    // TODO: proper error kind
    // TODO: proper origin
    Error(ErrorOrigin::OsLayer, ErrorKind::Unknown)
}

unsafe fn enable_debug_privilege() -> Result<()> {
    let process = GetCurrentProcess();
    let mut token = HANDLE(0);

    OpenProcessToken(process, TOKEN_ADJUST_PRIVILEGES, &mut token).map_err(conv_err)?;

    let mut luid = Default::default();

    let mut se_debug_name = *b"SeDebugPrivilege\0";

    LookupPrivilegeValueA(
        PCSTR(core::ptr::null_mut()),
        PCSTR(se_debug_name.as_mut_ptr()),
        &mut luid,
    )
    .map_err(conv_err)?;

    let new_privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };

    AdjustTokenPrivileges(
        token,
        false,
        Some(&new_privileges),
        std::mem::size_of_val(&new_privileges) as _,
        None,
        None,
    )
    .map_err(conv_err)
}

pub struct WindowsOs {
    info: OsInfo,
    cached_processes: Vec<ProcessInfo>,
    cached_modules: Vec<KernelModule>,
}

impl WindowsOs {
    pub fn new(args: &OsArgs) -> Result<Self> {
        match args.extra_args.get("elevate_token") {
            Some("off") | Some("OFF") | Some("Off") | Some("n") | Some("N") | Some("0") => {}
            _ => {
                unsafe { enable_debug_privilege() }?;
            }
        }

        Ok(Default::default())
    }
}

impl Clone for WindowsOs {
    fn clone(&self) -> Self {
        Self {
            info: self.info.clone(),
            cached_processes: vec![],
            cached_modules: vec![],
        }
    }
}

impl Default for WindowsOs {
    fn default() -> Self {
        let info = OsInfo {
            base: Address::NULL,
            size: 0,
            arch: ArchitectureIdent::X86(64, false),
        };

        Self {
            info,
            cached_modules: vec![],
            cached_processes: vec![],
        }
    }
}

impl Os for WindowsOs {
    type ProcessType<'a> = WindowsProcess;
    type IntoProcessType = WindowsProcess;

    /// Walks a process list and calls a callback for each process structure address
    ///
    /// The callback is fully opaque. We need this style so that C FFI can work seamlessly.
    fn process_address_list_callback(&mut self, callback: AddressCallback) -> Result<()> {
        let handle =
            Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(conv_err)?);

        let mut maybe_entry = MaybeUninit::<PROCESSENTRY32W>::uninit();

        unsafe {
            ptr::write(
                &mut (*maybe_entry.as_mut_ptr()).dwSize,
                size_of::<PROCESSENTRY32W>() as u32,
            );
        }

        let ptr = maybe_entry.as_mut_ptr();

        std::iter::once(unsafe { Process32FirstW(*handle, ptr) })
            .chain(std::iter::repeat_with(|| unsafe {
                Process32NextW(*handle, ptr)
            }))
            .take_while(|b| b.is_ok())
            .map(|_| unsafe { maybe_entry.assume_init() })
            .map(|p| {
                let address = Address::from(p.th32ProcessID as umem);
                let len = p.szExeFile.iter().take_while(|&&c| c != 0).count();

                let path = OsString::from_wide(&p.szExeFile[..len]);
                let path = path.to_string_lossy();
                let path = &*path;
                let name = path.rsplit_once('\\').map(|(_, end)| end).unwrap_or(path);

                self.cached_processes.push(ProcessInfo {
                    address,
                    pid: address.to_umem() as _,
                    state: ProcessState::Alive,
                    name: name.into(),
                    path: path.into(),
                    command_line: "".into(),
                    sys_arch: self.info.arch,
                    proc_arch: self.info.arch,
                    // dtb is not known/used here
                    dtb1: Address::invalid(),
                    dtb2: Address::invalid(),
                });

                address
            })
            .feed_into(callback);

        Ok(())
    }

    /// Find process information by its internal address
    fn process_info_by_address(&mut self, address: Address) -> Result<ProcessInfo> {
        self.cached_processes
            .iter()
            .find(|p| p.address == address)
            .cloned()
            .ok_or(Error(ErrorOrigin::OsLayer, ErrorKind::NotFound))
    }

    /// Construct a process by its info, borrowing the OS
    ///
    /// It will share the underlying memory resources
    fn process_by_info(&mut self, info: ProcessInfo) -> Result<Self::ProcessType<'_>> {
        WindowsProcess::try_new(info)
    }

    /// Construct a process by its info, consuming the OS
    ///
    /// This function will consume the Kernel instance and move its resources into the process
    fn into_process_by_info(mut self, info: ProcessInfo) -> Result<Self::IntoProcessType> {
        self.process_by_info(info)
    }

    /// Walks the OS module list and calls the provided callback for each module structure
    /// address
    ///
    /// # Arguments
    /// * `callback` - where to pass each matching module to. This is an opaque callback.
    fn module_address_list_callback(&mut self, mut callback: AddressCallback) -> Result<()> {
        /*self.cached_modules = procfs::modules()
        .map_err(|_| Error(ErrorOrigin::OsLayer, ErrorKind::UnableToReadDir))?
        .into_iter()
        .map(|(_, v)| v)
        .collect();*/

        (0..self.cached_modules.len())
            .map(Address::from)
            .take_while(|a| callback.call(*a))
            .for_each(|_| {});

        Ok(())
    }

    /// Retrieves a module by its structure address
    ///
    /// # Arguments
    /// * `address` - address where module's information resides in
    fn module_by_address(&mut self, _address: Address) -> Result<ModuleInfo> {
        /*self.cached_modules
        .iter()
        .skip(address.to_umem() as usize)
        .next()
        .map(|km| ModuleInfo {
            address,
            size: km.size as umem,
            base: Address::NULL,
            name: km
                .name
                .split("/")
                .last()
                .or(Some(""))
                .map(ReprCString::from)
                .unwrap(),
            arch: self.info.arch,
            path: km.name.clone().into(),
            parent_process: Address::INVALID,
        })
        .ok_or(Error(ErrorOrigin::OsLayer, ErrorKind::NotFound))*/

        todo!()
    }

    /// Retrieves address of the primary module structure of the process
    ///
    /// This will generally be for the initial executable that was run
    fn primary_module_address(&mut self) -> Result<Address> {
        Ok(self.module_by_name("ntoskrnl.exe")?.address)
    }

    /// Retrieves information for the primary module of the process
    ///
    /// This will generally be the initial executable that was run
    fn primary_module(&mut self) -> Result<ModuleInfo> {
        self.module_by_name("ntoskrnl.exe")
    }

    /// Retrieves a list of all imports of a given module
    fn module_import_list_callback(
        &mut self,
        _info: &ModuleInfo,
        _callback: ImportCallback,
    ) -> Result<()> {
        //memflow::os::util::module_import_list_callback(&mut self.virt_mem, info, callback)
        Err(Error(ErrorOrigin::OsLayer, ErrorKind::NotImplemented))
    }

    /// Retrieves a list of all exports of a given module
    fn module_export_list_callback(
        &mut self,
        _info: &ModuleInfo,
        _callback: ExportCallback,
    ) -> Result<()> {
        //memflow::os::util::module_export_list_callback(&mut self.virt_mem, info, callback)
        Err(Error(ErrorOrigin::OsLayer, ErrorKind::NotImplemented))
    }

    /// Retrieves a list of all sections of a given module
    fn module_section_list_callback(
        &mut self,
        _info: &ModuleInfo,
        _callback: SectionCallback,
    ) -> Result<()> {
        //memflow::os::util::module_section_list_callback(&mut self.virt_mem, info, callback)
        Err(Error(ErrorOrigin::OsLayer, ErrorKind::NotImplemented))
    }

    /// Retrieves the OS info
    fn info(&self) -> &OsInfo {
        &self.info
    }
}

impl OsKeyboard for WindowsOs {
    type KeyboardType<'a> = WindowsKeyboard;
    type IntoKeyboardType = WindowsKeyboard;

    fn keyboard(&mut self) -> memflow::error::Result<Self::KeyboardType<'_>> {
        Ok(WindowsKeyboard::new())
    }

    fn into_keyboard(self) -> memflow::error::Result<Self::IntoKeyboardType> {
        Ok(WindowsKeyboard::new())
    }
}
