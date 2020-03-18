use {
    super::*,
    zircon_object::{signal::Port, task::*, vm::*},
};

const ZX_PROP_NAME: u32 = 3;
const ZX_PROP_REGISTER_FS: u32 = 4;
const ZX_PROP_PROCESS_DEBUG_ADDR: u32 = 5;
const ZX_PROCESS_VDSO_BASE_ADDRESS: u32 = 6;
const ZX_PROP_PROCESS_BREAK_ON_LOAD: u32 = 7;
const ZX_MAX_NAME_LEN: u32 = 32;

impl Syscall<'_> {
    pub fn sys_object_get_property(
        &self,
        handle_value: HandleValue,
        property: u32,
        ptr: usize,
        buffer_size: u32,
    ) -> ZxResult<usize> {
        info!(
            "handle={:?}, property={:?}, buffer_ptr={:?}, size={:?}",
            handle_value, property, ptr, buffer_size
        );
        let object = self
            .thread
            .proc()
            .get_dyn_object_with_rights(handle_value, Rights::GET_PROPERTY)?;
        match property {
            ZX_PROP_NAME => {
                if buffer_size < ZX_MAX_NAME_LEN {
                    Err(ZxError::BUFFER_TOO_SMALL)
                } else {
                    let s = object.name();
                    info!("object_get_property: name is {}", s);
                    UserOutPtr::<u8>::from(ptr)
                        .write_cstring(s.as_str())
                        .expect("failed to write cstring");
                    Ok(0)
                }
            }
            ZX_PROP_PROCESS_DEBUG_ADDR => {
                if buffer_size < 8 {
                    return Err(ZxError::BUFFER_TOO_SMALL);
                }
                let debug_addr = self
                    .thread
                    .proc()
                    .get_object_with_rights::<Process>(handle_value, Rights::GET_PROPERTY)?
                    .get_debug_addr();
                UserOutPtr::<usize>::from(ptr).write(debug_addr)?;
                Ok(0)
            }
            ZX_PROCESS_VDSO_BASE_ADDRESS => {
                if buffer_size < 8 {
                    return Err(ZxError::BUFFER_TOO_SMALL);
                }
                let vdso_base = self
                    .thread
                    .proc()
                    .vmar()
                    .vdso_code_start()
                    .lock()
                    .unwrap_or(0);
                info!("vdso_base_addr: {:#X}", vdso_base);
                UserOutPtr::<usize>::from(ptr).write(vdso_base)?;
                Ok(0)
            }
            ZX_PROP_PROCESS_BREAK_ON_LOAD => {
                if buffer_size < 8 {
                    return Err(ZxError::BUFFER_TOO_SMALL);
                }
                let break_on_load = self
                    .thread
                    .proc()
                    .get_object_with_rights::<Process>(handle_value, Rights::GET_PROPERTY)?
                    .get_dyn_break_on_load();
                UserOutPtr::<usize>::from(ptr).write(break_on_load)?;
                Ok(0)
            }
            _ => {
                warn!("unknown property {} in OBJECT_GET_PROPERTY", property);
                Err(ZxError::INVALID_ARGS)
            }
        }
    }

    pub fn sys_object_set_property(
        &mut self,
        handle_value: HandleValue,
        property: u32,
        ptr: usize,
        buffer_size: u32,
    ) -> ZxResult<usize> {
        info!(
            "handle={:?}, property={:?}, buffer_ptr={:?}, size={:?}",
            handle_value, property, ptr, buffer_size
        );
        let object = self
            .thread
            .proc()
            .get_dyn_object_with_rights(handle_value, Rights::SET_PROPERTY)?;
        match property {
            ZX_PROP_NAME => {
                let length = if buffer_size > ZX_MAX_NAME_LEN {
                    (ZX_MAX_NAME_LEN - 1) as usize
                } else {
                    buffer_size as usize
                };
                let s = UserInPtr::<u8>::from(ptr).read_string(length)?;
                info!("object_set_property name: {}", s);
                object.set_name(&s);
                Ok(0)
            }
            ZX_PROP_PROCESS_DEBUG_ADDR => {
                if buffer_size < 8 {
                    return Err(ZxError::BUFFER_TOO_SMALL);
                }
                let addr = UserInPtr::<usize>::from(ptr).read()?;
                self.thread
                    .proc()
                    .get_object_with_rights::<Process>(handle_value, Rights::SET_PROPERTY)?
                    .set_debug_addr(addr);
                Ok(0)
            }
            ZX_PROP_REGISTER_FS => {
                if buffer_size < 8 {
                    return Err(ZxError::BUFFER_TOO_SMALL);
                }
                let thread = self.thread.proc().get_object::<Thread>(handle_value)?;
                assert!(Arc::ptr_eq(&thread, &self.thread));
                let fsbase = UserInPtr::<u64>::from(ptr).read()?;
                info!("to set fsbase as {:#x}", fsbase);
                self.regs.fsbase = fsbase as usize;
                Ok(0)
            }
            ZX_PROP_PROCESS_BREAK_ON_LOAD => {
                if buffer_size < 8 {
                    return Err(ZxError::BUFFER_TOO_SMALL);
                }
                let addr = UserInPtr::<usize>::from(ptr).read()?;
                self.thread
                    .proc()
                    .get_object_with_rights::<Process>(handle_value, Rights::SET_PROPERTY)?
                    .set_dyn_break_on_load(addr);
                Ok(0)
            }
            _ => {
                warn!("unknown property {} in OBJECT_SET_PROPERTY", property);
                Err(ZxError::INVALID_ARGS)
            }
        }
    }

    pub async fn sys_object_wait_one(
        &self,
        handle: HandleValue,
        signals: u32,
        deadline: u64,
        mut observed: UserOutPtr<Signal>,
    ) -> ZxResult<usize> {
        info!(
            "object.wait_one: handle={:?}, signals={:#x?}, deadline={:#x?}, observed={:#x?}",
            handle, signals, deadline, observed
        );
        let signals = Signal::from_bits(signals).ok_or(ZxError::INVALID_ARGS)?;
        let object = self
            .thread
            .proc()
            .get_dyn_object_with_rights(handle, Rights::WAIT)?;
        observed.write(object.wait_signal_async(signals).await)?;
        Ok(0)
    }

    pub fn sys_object_get_info(
        &self,
        handle: HandleValue,
        topic: u32,
        buffer: usize,
        _buffer_size: usize,
        _actual: UserOutPtr<usize>,
        _avail: UserOutPtr<usize>,
    ) -> ZxResult<usize> {
        match ZxInfo::from(topic) {
            ZxInfo::InfoProcess => {
                let proc = self
                    .thread
                    .proc()
                    .get_object_with_rights::<Process>(handle, Rights::INSPECT)?;
                UserOutPtr::<ProcessInfo>::from(buffer).write(proc.get_info())?;
            }
            ZxInfo::InfoVmar => {
                let vmar = self
                    .thread
                    .proc()
                    .get_object_with_rights::<VmAddressRegion>(handle, Rights::INSPECT)?;
                UserOutPtr::<VmarInfo>::from(buffer).write(vmar.get_info())?;
            }
            ZxInfo::InfoHandleBasic => {
                let info = self.thread.proc().get_handle_info(handle)?;
                info!("basic info: {:?}", info);
                UserOutPtr::<HandleBasicInfo>::from(buffer).write(info)?;
            }
            _ => {
                warn!("not supported info topic");
                return Err(ZxError::NOT_SUPPORTED);
            }
        }
        Ok(0)
    }

    pub fn sys_object_signal_peer(
        &self,
        handle_value: HandleValue,
        clear_mask: u32,
        set_mask: u32,
    ) -> ZxResult<usize> {
        info!(
            "object.signal_peer: handle_value = {}, clear_mask = {:#x}, set_mask = {:#x}",
            handle_value, clear_mask, set_mask
        );
        let object = self
            .thread
            .proc()
            .get_dyn_object_with_rights(handle_value, Rights::SIGNAL_PEER)?;
        let clear_signal = Signal::verify_user_signal(clear_mask)?;
        let set_signal = Signal::verify_user_signal(set_mask)?;
        object.user_signal_peer(clear_signal, set_signal)?;
        Ok(0)
    }

    pub fn sys_object_wait_async(
        &self,
        handle_value: HandleValue,
        port_handle_value: HandleValue,
        key: u64,
        signals: u32,
        options: u32,
    ) -> ZxResult<usize> {
        let signals = Signal::from_bits(signals).ok_or(ZxError::INVALID_ARGS)?;
        info!(
            "object.wait_async: handle={}, port={}, key={:#x}, signal={:?}, options={:#X}",
            handle_value, port_handle_value, key, signals, options
        );
        if options != 0 {
            unimplemented!()
        }
        // TODO filter `options`
        let proc = self.thread.proc();
        let object = proc.get_dyn_object_with_rights(handle_value, Rights::WAIT)?;
        let port = proc.get_object_with_rights::<Port>(port_handle_value, Rights::WRITE)?;
        object.send_signal_to_port_async(signals, &port, key);
        Ok(0)
    }
}

#[repr(u32)]
enum ZxInfo {
    InfoNone = 0u32,
    InfoHandleValid = 1u32,
    InfoHandleBasic = 2u32,
    InfoProcess = 3u32,
    InfoProcessThreads = 4u32,
    InfoVmar = 7u32,
    InfoJobChildren = 8u32,
    InfoJobProcess = 9u32,
    InfoThread = 10u32,
    InfoThreadExceptionReport = 11u32,
    InfoTaskStats = 12u32,
    InfoProcessMaps = 13u32,
    InfoProcessVmos = 14u32,
    InfoThreadStats = 15u32,
    InfoCpuStats = 16u32,
    InfoKmemStats = 17u32,
    InfoResource = 18u32,
    InfoHandleCount = 19u32,
    InfoBti = 20u32,
    InfoProcessHandleStats = 21u32,
    InfoSocket = 22u32,
    InfoVmo = 23u32,
    InfoJob = 24u32,
    InfoTimer = 26u32,
    InfoStream = 27u32,
    Unknown,
}

impl From<u32> for ZxInfo {
    fn from(number: u32) -> Self {
        match number {
            0 => ZxInfo::InfoNone,
            1 => ZxInfo::InfoHandleValid,
            2 => ZxInfo::InfoHandleBasic,
            3 => ZxInfo::InfoProcess,
            4 => ZxInfo::InfoProcessThreads,
            7 => ZxInfo::InfoVmar,
            8 => ZxInfo::InfoJobChildren,
            9 => ZxInfo::InfoJobProcess,
            10 => ZxInfo::InfoThread,
            11 => ZxInfo::InfoThreadExceptionReport,
            12 => ZxInfo::InfoTaskStats,
            13 => ZxInfo::InfoProcessMaps,
            14 => ZxInfo::InfoProcessVmos,
            15 => ZxInfo::InfoThreadStats,
            16 => ZxInfo::InfoCpuStats,
            17 => ZxInfo::InfoKmemStats,
            18 => ZxInfo::InfoResource,
            19 => ZxInfo::InfoHandleCount,
            20 => ZxInfo::InfoBti,
            21 => ZxInfo::InfoProcessHandleStats,
            22 => ZxInfo::InfoSocket,
            23 => ZxInfo::InfoVmo,
            24 => ZxInfo::InfoJob,
            26 => ZxInfo::InfoTimer,
            27 => ZxInfo::InfoStream,
            _ => ZxInfo::Unknown,
        }
    }
}