use crate::{native::*, Error, Event};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

/// One bounded, ordered worker per engine. No worker calls application callbacks.
pub(crate) struct Jobs {
    sender: mpsc::SyncSender<(RequestId, NativeRequest)>,
    receiver: mpsc::Receiver<Event>,
    cancelled: Arc<AtomicBool>,
    pending: usize,
}
impl Jobs {
    pub fn new() -> Result<Self, Error> {
        let (sender, requests) = mpsc::sync_channel::<(RequestId, NativeRequest)>(16);
        let (results, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::Builder::new()
            .name("rusterize-io".into())
            .spawn(move || {
                while let Ok((id, request)) = requests.recv() {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let result = execute(request);
                    if stop.load(Ordering::Acquire)
                        || results.send(Event::NativeResult { id, result }).is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| Error(format!("cannot start resource worker: {e}")))?;
        Ok(Self {
            sender,
            receiver,
            cancelled,
            pending: 0,
        })
    }
    pub fn submit(&mut self, id: RequestId, request: NativeRequest) -> Result<(), Error> {
        // Also bound completed results that have not yet been consumed.
        if self.pending >= 16 {
            return Err(Error("resource queue is full (16 requests)".into()));
        }
        self.sender
            .try_send((id, request))
            .map_err(|_| Error("resource worker queue unavailable".into()))?;
        self.pending += 1;
        Ok(())
    }
    pub fn poll(&mut self) -> Option<Event> {
        let event = self.receiver.try_recv().ok()?;
        self.pending -= 1;
        Some(event)
    }
    pub fn pending(&self) -> bool {
        self.pending != 0
    }
}
impl Drop for Jobs {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

pub(crate) fn handles(request: &NativeRequest) -> bool {
    match request {
        #[cfg(feature = "files")]
        NativeRequest::Files(request) => !matches!(request, crate::files::FileRequest::Pick(_)),
        #[cfg(feature = "audio")]
        NativeRequest::DecodeAudio(_) => true,
        #[cfg(feature = "image-codecs")]
        NativeRequest::DecodeImage(_) => true,
        _ => false,
    }
}
fn execute(request: NativeRequest) -> Result<NativeResponse, Error> {
    match request {
        #[cfg(feature = "files")]
        NativeRequest::Files(request) => crate::files::execute(&request),
        #[cfg(feature = "audio")]
        NativeRequest::DecodeAudio(bytes) => {
            crate::audio::Clip::decode(bytes).map(NativeResponse::Audio)
        }
        #[cfg(feature = "image-codecs")]
        NativeRequest::DecodeImage(bytes) => {
            crate::Image::decode(&bytes).map(NativeResponse::Image)
        }
        _ => Err(Error(
            "operation is not a background resource request".into(),
        )),
    }
}
