// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Capturing the audio other applications play with a Core Audio process
//! tap, which macOS 14.2 added. A tap is read through a private aggregate
//! device that exists only while Euphoria listens. The audio goes to the
//! sink and nowhere else.

use std::ffi::{CStr, c_void};
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_audio::{
    AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop,
    AudioHardwareCreateAggregateDevice, AudioHardwareDestroyAggregateDevice,
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, CATapDescription, kAudioDevicePropertyDeviceUID,
    kAudioHardwarePropertyDefaultSystemOutputDevice, kAudioHardwarePropertyProcessObjectList,
    kAudioHardwarePropertyTranslatePIDToProcessObject, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioProcessPropertyIsRunningOutput, kAudioProcessPropertyPID, kAudioTapPropertyFormat,
};
use objc2_core_audio_types::{AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};

use super::capture::{AudioCapture, AudioSink, CaptureError, ListenSource};

const MINIMUM_MACOS: &str = "Listening to other applications needs macOS 14.2 or later.";
const NON_INTERLEAVED: u32 = 1 << 5;
const NO_ERROR: i32 = 0;

type CreateTap = unsafe extern "C-unwind" fn(*const AnyObject, *mut AudioObjectID) -> i32;
type DestroyTap = unsafe extern "C-unwind" fn(AudioObjectID) -> i32;
type IoBlock = block2::Block<
    dyn Fn(
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
    ),
>;

struct TapFunctions {
    create: CreateTap,
    destroy: DestroyTap,
}

fn tap_functions() -> Option<TapFunctions> {
    let find = |name: &CStr| {
        // SAFETY: dlsym looks a symbol up by name and returns null when it is absent.
        let address = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
        (!address.is_null()).then_some(address)
    };
    let create = find(c"AudioHardwareCreateProcessTap")?;
    let destroy = find(c"AudioHardwareDestroyProcessTap")?;
    // SAFETY: both symbols have these signatures in macOS 14.2 and later.
    unsafe {
        Some(TapFunctions {
            create: std::mem::transmute::<*mut c_void, CreateTap>(create),
            destroy: std::mem::transmute::<*mut c_void, DestroyTap>(destroy),
        })
    }
}

fn address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn get_value<Value: Default>(
    object: AudioObjectID,
    selector: u32,
    qualifier: Option<&u32>,
) -> Option<Value> {
    let mut value = Value::default();
    let mut size = size_of::<Value>() as u32;
    let property = address(selector);
    let (qualifier_size, qualifier_pointer) = qualifier.map_or((0, std::ptr::null()), |value| {
        (
            size_of::<u32>() as u32,
            value as *const u32 as *const c_void,
        )
    });
    // SAFETY: the pointers refer to live locals of the sizes passed with them.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&property),
            qualifier_size,
            qualifier_pointer,
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    (status == NO_ERROR).then_some(value)
}

fn process_object_of(process_id: i32) -> Option<AudioObjectID> {
    let object: AudioObjectID = get_value(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyTranslatePIDToProcessObject,
        Some(&(process_id as u32)),
    )?;
    (object != 0).then_some(object)
}

fn process_objects() -> Vec<AudioObjectID> {
    let property = address(kAudioHardwarePropertyProcessObjectList);
    let mut size = 0_u32;
    // SAFETY: the pointers refer to live locals.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&property),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != NO_ERROR || size == 0 {
        return Vec::new();
    }
    let mut objects = vec![0_u32; size as usize / size_of::<u32>()];
    // SAFETY: the buffer holds `size` bytes.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&property),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(objects.as_mut_ptr().cast()).expect("a vector has an address"),
        )
    };
    if status == NO_ERROR {
        objects.truncate(size as usize / size_of::<u32>());
        objects
    } else {
        Vec::new()
    }
}

fn process_name(process_id: i32) -> Option<String> {
    let mut name = [0_u8; 256];
    // SAFETY: the buffer is as long as the length passed.
    let length =
        unsafe { libc::proc_name(process_id, name.as_mut_ptr().cast(), name.len() as u32) };
    (length > 0).then(|| String::from_utf8_lossy(&name[..length as usize]).into_owned())
}

fn default_output_uid() -> Option<Retained<NSString>> {
    let device: AudioObjectID = get_value(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyDefaultSystemOutputDevice,
        None,
    )?;
    let mut uid: *mut NSString = std::ptr::null_mut();
    let mut size = size_of::<*mut NSString>() as u32;
    let property = address(kAudioDevicePropertyDeviceUID);
    // SAFETY: Core Audio writes a retained CFString, which is an NSString.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            NonNull::from(&property),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut uid).cast(),
        )
    };
    if status != NO_ERROR {
        return None;
    }
    // SAFETY: the pointer is the retained string just returned, or null.
    unsafe { Retained::from_raw(uid) }
}

fn macos_at_least_14_2() -> bool {
    tap_functions().is_some() && objc2::runtime::AnyClass::get(c"CATapDescription").is_some()
}

struct Running {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    procedure: AudioDeviceIOProcID,
    _block: RcBlock<
        dyn Fn(
            NonNull<AudioTimeStamp>,
            NonNull<AudioBufferList>,
            NonNull<AudioTimeStamp>,
            NonNull<AudioBufferList>,
            NonNull<AudioTimeStamp>,
        ),
    >,
    destroy_tap: DestroyTap,
}

// SAFETY: the block and the ids are only used to stop what `start` created, and
// Core Audio allows that from any thread.
unsafe impl Send for Running {}

/// Listens to the audio of other applications with a Core Audio process tap.
#[derive(Default)]
pub struct ProcessTapCapture {
    running: Option<Running>,
}

impl ProcessTapCapture {
    /// A capture that is not listening yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// True when this macOS can tap the audio of other applications.
    pub fn is_supported() -> bool {
        macos_at_least_14_2()
    }
}

fn denied(detail: &str) -> CaptureError {
    CaptureError::Failed(format!(
        "Euphoria could not start listening ({detail}). Allow it under System Settings > Privacy & Security > Screen & System Audio Recording."
    ))
}

impl AudioCapture for ProcessTapCapture {
    fn sources(&self) -> Vec<ListenSource> {
        let own = std::process::id() as i32;
        let mut sources: Vec<ListenSource> = Vec::new();
        for object in process_objects() {
            let Some(process_id) = get_value::<i32>(object, kAudioProcessPropertyPID, None) else {
                continue;
            };
            let playing = get_value::<u32>(object, kAudioProcessPropertyIsRunningOutput, None)
                .is_some_and(|flag| flag != 0);
            if process_id == own || !playing {
                continue;
            }
            let Some(name) = process_name(process_id) else {
                continue;
            };
            if sources.iter().any(|source| source.label() == name) {
                continue;
            }
            sources.push(ListenSource::Process { process_id, name });
        }
        sources.sort_by(|left, right| left.label().cmp(right.label()));
        sources
    }

    fn start(
        &mut self,
        source: &ListenSource,
        sink: AudioSink,
    ) -> Result<NonZeroU32, CaptureError> {
        self.stop();
        let functions =
            tap_functions().ok_or_else(|| CaptureError::Unsupported(MINIMUM_MACOS.into()))?;
        if objc2::runtime::AnyClass::get(c"CATapDescription").is_none() {
            return Err(CaptureError::Unsupported(MINIMUM_MACOS.into()));
        }

        let description = match source {
            ListenSource::AllAudio => {
                let own = process_object_of(std::process::id() as i32);
                let excluded: Vec<Retained<NSNumber>> = own
                    .into_iter()
                    .map(|object| NSNumber::new_u32(object))
                    .collect();
                let excluded = NSArray::from_retained_slice(&excluded);
                // SAFETY: the class exists, as checked above, and the argument is an array of numbers.
                unsafe {
                    CATapDescription::initStereoGlobalTapButExcludeProcesses(
                        CATapDescription::alloc(),
                        &excluded,
                    )
                }
            }
            ListenSource::Process { process_id, name } => {
                let object = process_object_of(*process_id).ok_or_else(|| {
                    CaptureError::Failed(format!("{name} is not playing audio right now."))
                })?;
                let included = [NSNumber::new_u32(object)];
                let included = NSArray::from_retained_slice(&included);
                // SAFETY: as above.
                unsafe {
                    CATapDescription::initStereoMixdownOfProcesses(
                        CATapDescription::alloc(),
                        &included,
                    )
                }
            }
        };
        // SAFETY: plain property setters on the description just made.
        unsafe {
            description.setPrivate(true);
            description.setName(&NSString::from_str("Euphoria Live"));
        }

        let mut tap: AudioObjectID = 0;
        // SAFETY: the function was resolved above and the pointers are live.
        let status = unsafe { (functions.create)(Retained::as_ptr(&description).cast(), &mut tap) };
        if status != NO_ERROR || tap == 0 {
            return Err(denied(&format!("the tap failed with status {status}")));
        }

        let format: Option<AudioStreamBasicDescription> = get_value_of_format(tap);
        let Some(format) = format else {
            // SAFETY: the tap was just created.
            unsafe { (functions.destroy)(tap) };
            return Err(CaptureError::Failed("the tap has no audio format".into()));
        };
        let sample_rate = NonZeroU32::new(format.mSampleRate as u32)
            .unwrap_or(NonZeroU32::new(48_000).expect("48000 is not zero"));
        let channels = format.mChannelsPerFrame.max(1) as usize;
        let interleaved = format.mFormatFlags & NON_INTERLEAVED == 0;

        // SAFETY: the description lives until here and its UUID is a plain object.
        let tap_uuid: Retained<NSUUID> = unsafe { description.UUID() };
        let tap_uid = tap_uuid.UUIDString();

        let aggregate_uid = NSUUID::new().UUIDString();
        let aggregate = match create_aggregate(&aggregate_uid, &tap_uid) {
            Ok(aggregate) => aggregate,
            Err(error) => {
                // SAFETY: the tap was just created.
                unsafe { (functions.destroy)(tap) };
                return Err(error);
            }
        };

        let state = Mutex::new((sink, Vec::<f32>::new()));
        let block = RcBlock::new(
            move |_now: NonNull<AudioTimeStamp>,
                  input: NonNull<AudioBufferList>,
                  _input_time: NonNull<AudioTimeStamp>,
                  _output: NonNull<AudioBufferList>,
                  _output_time: NonNull<AudioTimeStamp>| {
                let Ok(mut guard) = state.try_lock() else {
                    return;
                };
                let (sink, scratch) = &mut *guard;
                // SAFETY: Core Audio passes a valid buffer list for the duration of the call.
                downmix(unsafe { input.as_ref() }, channels, interleaved, scratch);
                if !scratch.is_empty() {
                    sink(scratch);
                }
            },
        );

        let mut procedure: AudioDeviceIOProcID = None;
        // SAFETY: the block lives in `Running` until the procedure is destroyed.
        let status = unsafe {
            objc2_core_audio::AudioDeviceCreateIOProcIDWithBlock(
                NonNull::from(&mut procedure),
                aggregate,
                None,
                &*block as *const IoBlock as *mut _,
            )
        };
        if status != NO_ERROR || procedure.is_none() {
            // SAFETY: both objects were just created.
            unsafe {
                AudioHardwareDestroyAggregateDevice(aggregate);
                (functions.destroy)(tap);
            }
            return Err(denied(&format!(
                "the audio procedure failed with status {status}"
            )));
        }
        // SAFETY: the device and procedure were just created.
        let status = unsafe { AudioDeviceStart(aggregate, procedure) };
        if status != NO_ERROR {
            // SAFETY: as above.
            unsafe {
                AudioDeviceDestroyIOProcID(aggregate, procedure);
                AudioHardwareDestroyAggregateDevice(aggregate);
                (functions.destroy)(tap);
            }
            return Err(denied(&format!("starting failed with status {status}")));
        }
        self.running = Some(Running {
            tap,
            aggregate,
            procedure,
            _block: block,
            destroy_tap: functions.destroy,
        });
        Ok(sample_rate)
    }

    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            // SAFETY: these objects were created by `start` and are destroyed once.
            unsafe {
                AudioDeviceStop(running.aggregate, running.procedure);
                AudioDeviceDestroyIOProcID(running.aggregate, running.procedure);
                AudioHardwareDestroyAggregateDevice(running.aggregate);
                (running.destroy_tap)(running.tap);
            }
        }
    }
}

impl Drop for ProcessTapCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn get_value_of_format(tap: AudioObjectID) -> Option<AudioStreamBasicDescription> {
    let mut format = std::mem::MaybeUninit::<AudioStreamBasicDescription>::zeroed();
    let mut size = size_of::<AudioStreamBasicDescription>() as u32;
    let property = address(kAudioTapPropertyFormat);
    // SAFETY: the buffer is as large as the format.
    let status = unsafe {
        AudioObjectGetPropertyData(
            tap,
            NonNull::from(&property),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(format.as_mut_ptr().cast()).expect("a local has an address"),
        )
    };
    // SAFETY: zeroed is a valid value of this plain struct, and Core Audio filled it on success.
    (status == NO_ERROR).then(|| unsafe { format.assume_init() })
}

fn create_aggregate(
    aggregate_uid: &NSString,
    tap_uid: &NSString,
) -> Result<AudioObjectID, CaptureError> {
    let output = default_output_uid()
        .ok_or_else(|| CaptureError::Failed("there is no audio output device".into()))?;
    let uid_key = NSString::from_str("uid");
    let drift_key = NSString::from_str("drift");
    let yes = NSNumber::new_bool(true);
    let no = NSNumber::new_bool(false);
    let name = NSString::from_str("Euphoria Live");
    let sub_device: Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::from_slices(&[&*uid_key], &[object(&*output)]);
    let tap_entry: Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::from_slices(&[&*uid_key, &*drift_key], &[object(tap_uid), object(&*yes)]);
    let sub_devices = NSArray::from_retained_slice(&[sub_device]);
    let taps = NSArray::from_retained_slice(&[tap_entry]);
    let keys: Vec<Retained<NSString>> = [
        "name",
        "uid",
        "master",
        "private",
        "stacked",
        "tapautostart",
        "subdevices",
        "taps",
    ]
    .iter()
    .map(|text| NSString::from_str(text))
    .collect();
    let key_references: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
    let description: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
        &key_references,
        &[
            object(&*name),
            object(aggregate_uid),
            object(&*output),
            object(&*yes),
            object(&*no),
            object(&*yes),
            object(&*sub_devices),
            object(&*taps),
        ],
    );
    let mut aggregate: AudioObjectID = 0;
    // SAFETY: an NSDictionary is a CFDictionary, and the pointer is live.
    let status = unsafe {
        AudioHardwareCreateAggregateDevice(
            &*(Retained::as_ptr(&description) as *const _),
            NonNull::from(&mut aggregate),
        )
    };
    if status != NO_ERROR || aggregate == 0 {
        return Err(denied(&format!(
            "the listening device failed with status {status}"
        )));
    }
    Ok(aggregate)
}

fn downmix(list: &AudioBufferList, channels: usize, interleaved: bool, mono: &mut Vec<f32>) {
    mono.clear();
    let buffer_count = list.mNumberBuffers as usize;
    // SAFETY: the list holds `mNumberBuffers` buffers after its header.
    let buffers = unsafe { std::slice::from_raw_parts(list.mBuffers.as_ptr(), buffer_count) };
    if interleaved {
        let Some(buffer) = buffers.first() else {
            return;
        };
        if buffer.mData.is_null() {
            return;
        }
        let count = buffer.mDataByteSize as usize / size_of::<f32>();
        // SAFETY: the buffer holds `mDataByteSize` bytes of 32 bit float samples.
        let samples = unsafe { std::slice::from_raw_parts(buffer.mData as *const f32, count) };
        for frame in samples.chunks_exact(channels) {
            mono.push(frame.iter().sum::<f32>() / channels as f32);
        }
    } else {
        let frames = buffers
            .iter()
            .map(|buffer| buffer.mDataByteSize as usize / size_of::<f32>())
            .min()
            .unwrap_or(0);
        mono.resize(frames, 0.0);
        for buffer in buffers {
            if buffer.mData.is_null() {
                continue;
            }
            // SAFETY: each buffer holds at least `frames` float samples.
            let samples = unsafe { std::slice::from_raw_parts(buffer.mData as *const f32, frames) };
            for (target, sample) in mono.iter_mut().zip(samples) {
                *target += sample / buffers.len() as f32;
            }
        }
    }
}

fn object<Value: objc2::Message>(value: &Value) -> &AnyObject {
    // SAFETY: every Objective-C object is an AnyObject.
    unsafe { &*(value as *const Value as *const AnyObject) }
}
