//! The Windows side: Aurora's window's System Media Transport Controls.
//!
//! A desktop app asks for the controls of its own window through
//! `ISystemMediaTransportControlsInterop`; `GetForCurrentView` is for packaged UWP views
//! only. Everything here runs on the `media-keys` thread, which joins the multi-threaded
//! apartment first, and the button handlers run on the thread pool.

use std::sync::Arc;

use windows::core::{factory, Interface, HSTRING};
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Storage::{IStorageFile, StorageFile};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::{
    ISystemMediaTransportControlsInterop, RoInitialize, RO_INIT_MULTITHREADED,
};

use super::{Artwork, Button, Changes, Controls, Playback, Shown, Timeline};

pub struct Smtc {
    controls: SystemMediaTransportControls,
}

/// 100-nanosecond ticks, as `TimeSpan` counts them.
fn span(secs: f64) -> TimeSpan {
    TimeSpan {
        Duration: (secs.max(0.0) * 10_000_000.0) as i64,
    }
}

impl Smtc {
    /// The controls for `hwnd`, with a press sent to `pressed`. Call on the thread that
    /// will go on using them.
    pub fn new(
        hwnd: isize,
        pressed: Arc<dyn Fn(Button) + Send + Sync>,
    ) -> windows::core::Result<Self> {
        // `S_FALSE` (already joined) is fine; a thread already in the other apartment
        // fails, which is only possible if this were called somewhere it should not be.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.or_else(|e| {
            if e.code() == windows::Win32::Foundation::RPC_E_CHANGED_MODE {
                Ok(())
            } else {
                Err(e)
            }
        })?;
        let interop =
            factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        let controls: SystemMediaTransportControls =
            unsafe { interop.GetForWindow(HWND(hwnd as *mut core::ffi::c_void))? };
        controls.SetIsEnabled(false)?;
        controls.SetIsPlayEnabled(true)?;
        controls.SetIsPauseEnabled(true)?;
        controls.SetIsStopEnabled(false)?;

        let on_button = Arc::clone(&pressed);
        controls.ButtonPressed(&TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let button = match args.Button()? {
                SystemMediaTransportControlsButton::Play => Button::Play,
                SystemMediaTransportControlsButton::Pause => Button::Pause,
                SystemMediaTransportControlsButton::Next
                | SystemMediaTransportControlsButton::ChannelUp => Button::Next,
                SystemMediaTransportControlsButton::Previous
                | SystemMediaTransportControlsButton::ChannelDown => Button::Previous,
                _ => return Ok(()),
            };
            on_button(button);
            Ok(())
        }))?;

        let on_seek = pressed;
        controls.PlaybackPositionChangeRequested(&TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                let at = args.RequestedPlaybackPosition()?;
                on_seek(Button::Seek {
                    position_secs: at.Duration as f64 / 10_000_000.0,
                });
            }
            Ok(())
        }))?;

        Ok(Self { controls })
    }

    fn playback(&self, p: Playback) -> windows::core::Result<()> {
        self.controls.SetIsEnabled(p != Playback::Closed)?;
        self.controls.SetPlaybackStatus(match p {
            Playback::Closed => MediaPlaybackStatus::Closed,
            Playback::Changing => MediaPlaybackStatus::Changing,
            Playback::Playing => MediaPlaybackStatus::Playing,
            Playback::Paused => MediaPlaybackStatus::Paused,
        })
    }

    fn shown(&self, s: &Shown) -> windows::core::Result<()> {
        let d = self.controls.DisplayUpdater()?;
        d.ClearAll()?;
        d.SetType(MediaPlaybackType::Video)?;
        let v = d.VideoProperties()?;
        v.SetTitle(&HSTRING::from(s.title.as_str()))?;
        if let Some(sub) = &s.subtitle {
            v.SetSubtitle(&HSTRING::from(sub.as_str()))?;
        }
        if let Some(art) = &s.artwork {
            // A picture that will not load is not worth losing the title over.
            match thumbnail(art) {
                Ok(t) => d.SetThumbnail(&t)?,
                Err(e) => tracing::debug!("no picture in the media overlay: {e}"),
            }
        }
        d.Update()
    }

    fn timeline(&self, t: Option<Timeline>) -> windows::core::Result<()> {
        let p = SystemMediaTransportControlsTimelineProperties::new()?;
        // All zero takes the bar away, which is what live TV wants.
        let (position, end) = t.map_or((0.0, 0.0), |t| (t.position_secs, t.duration_secs));
        p.SetStartTime(span(0.0))?;
        p.SetMinSeekTime(span(0.0))?;
        p.SetEndTime(span(end))?;
        p.SetMaxSeekTime(span(end))?;
        p.SetPosition(span(position))?;
        self.controls.UpdateTimelineProperties(&p)
    }
}

fn thumbnail(art: &Artwork) -> windows::core::Result<RandomAccessStreamReference> {
    match art {
        Artwork::File(path) => {
            let file =
                StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.get()?;
            RandomAccessStreamReference::CreateFromFile(&file.cast::<IStorageFile>()?)
        }
        Artwork::Url(url) => RandomAccessStreamReference::CreateFromUri(&Uri::CreateUri(
            &HSTRING::from(url.as_str()),
        )?),
    }
}

impl Controls for Smtc {
    fn apply(&mut self, c: &Changes) -> Result<(), String> {
        let go = || -> windows::core::Result<()> {
            if let Some(s) = &c.shown {
                self.shown(s)?;
            }
            if let Some(on) = c.channels {
                self.controls.SetIsNextEnabled(on)?;
                self.controls.SetIsPreviousEnabled(on)?;
                self.controls.SetIsChannelUpEnabled(on)?;
                self.controls.SetIsChannelDownEnabled(on)?;
            }
            if let Some(t) = c.timeline {
                self.timeline(t)?;
            }
            // Last, so the overlay appears with its title already set.
            if let Some(p) = c.playback {
                self.playback(p)?;
            }
            Ok(())
        };
        go().map_err(|e| e.to_string())
    }
}

impl Drop for Smtc {
    fn drop(&mut self) {
        let _ = self.controls.SetIsEnabled(false);
    }
}
