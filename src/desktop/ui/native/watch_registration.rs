use crate::watch_queue::{Record, queue};
use crate::watch_win32::*;
use std::sync::mpsc::SyncSender;

pub struct Registration {
    pub id: usize,
    pub handle: isize,
    pub sequence: u64,
    pub stopping: bool,
    pub stopped: Vec<SyncSender<()>>,
    recursive: bool,
    buffer: [u32; 16384],
    operation: Overlapped,
}

impl Registration {
    pub fn new(id: usize, path: &[u16], recursive: bool, port: isize) -> Result<Box<Self>, u32> {
        let handle = open_path(path, 1, 0x4200_0000)?;
        if unsafe { CreateIoCompletionPort(handle, port, id, 0) } == 0 {
            let error = unsafe { GetLastError() };
            close(handle);
            return Err(error);
        }
        let mut registration = Box::new(Self {
            id,
            handle,
            sequence: 0,
            stopping: false,
            stopped: Vec::new(),
            recursive,
            buffer: [0; 16384],
            operation: Overlapped::default(),
        });
        registration.arm()?;
        Ok(registration)
    }

    pub fn arm(&mut self) -> Result<(), u32> {
        self.operation = Overlapped::default();
        let queued = unsafe {
            ReadDirectoryChangesW(
                self.handle,
                self.buffer.as_mut_ptr().cast(),
                65536,
                i32::from(self.recursive),
                0x15f,
                std::ptr::null_mut(),
                &mut self.operation,
                std::ptr::null(),
            )
        };
        if queued == 0 {
            return Err(unsafe { GetLastError() });
        }
        Ok(())
    }

    pub fn cancel(&mut self) {
        self.stopping = true;
        unsafe { CancelIoEx(self.handle, &mut self.operation) };
    }

    pub fn record(&mut self, action: u32, path: Vec<u16>, error: u32, loss: bool) {
        self.sequence += 1;
        queue().lock().unwrap().push(Record {
            id: self.id,
            sequence: self.sequence,
            action,
            path,
            error,
            loss,
        });
    }

    pub fn decode(&mut self, length: u32) {
        if length == 0 || length > 65536 {
            self.record(0, Vec::new(), ENUM_DIR, true);
            return;
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(self.buffer.as_ptr().cast::<u8>(), length as usize)
        };
        let mut offset = 0;
        let mut records = Vec::new();
        loop {
            if offset + 12 > bytes.len() {
                self.record(0, Vec::new(), ENUM_DIR, true);
                return;
            }
            let next = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let action = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
            let count =
                u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()) as usize;
            if count % 2 != 0 || offset + 12 + count > bytes.len() {
                self.record(0, Vec::new(), ENUM_DIR, true);
                return;
            }
            let path = bytes[offset + 12..offset + 12 + count]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            records.push((action, path));
            if next == 0 {
                break;
            }
            if next < 12 + count || next % 4 != 0 || offset + next >= bytes.len() {
                self.record(0, Vec::new(), ENUM_DIR, true);
                return;
            }
            offset += next;
        }
        for (action, path) in records {
            self.record(action, path, 0, false);
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        close(self.handle);
        for reply in self.stopped.drain(..) {
            let _ = reply.send(());
        }
    }
}
