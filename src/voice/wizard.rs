//! Asynchronous background worker for the Voice Trigger setup wizard.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::voice::features::extract_mfcc;
use crate::voice::listener::WizardCapture;
use crate::voice::matcher::{enroll, match_utterance, EnrollmentError, VoiceTemplates};

/// Commands sent from the UI to the wizard worker.
#[derive(Debug, Clone)]
pub enum WizardCmd {
    Start { device_hint: String, sensitivity: f32 },
    RedoTake(usize),
    ResetEnrollment,
    UpdateSensitivity(f32),
    Stop,
}

/// Events sent from the wizard worker to the UI.
#[derive(Debug, Clone)]
pub enum WizardEvent {
    CaptureReady,
    CaptureError(String),
    TakeRecorded { index: usize, next_take: usize },
    EnrollmentSuccess,
    EnrollmentOutlier { take_index: usize },
    EnrollmentFailed(String),
    TestResult { matched: bool, distance: f32, threshold: f32 },
}

/// Controller handle used by egui UI to communicate with background wizard worker.
pub struct VoiceWizardController {
    cmd_tx: Sender<WizardCmd>,
    event_rx: Arc<std::sync::Mutex<Receiver<WizardEvent>>>,
    level: Arc<AtomicU32>,
}

impl Default for VoiceWizardController {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceWizardController {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = channel::<WizardCmd>();
        let (event_tx, event_rx) = channel::<WizardEvent>();

        let level = Arc::new(AtomicU32::new(0));
        let level_worker = Arc::clone(&level);

        std::thread::spawn(move || {
            let mut capture: Option<WizardCapture> = None;
            let mut takes: [Option<Vec<Vec<f32>>>; 6] = [None, None, None, None, None, None];
            let mut current_take = 0usize;
            let mut testing = false;
            let mut sensitivity = 1.0f32;
            let mut cached_templates: Option<VoiceTemplates> = None;

            loop {
                // Drain any commands from UI
                loop {
                    let cmd = match cmd_rx.try_recv() {
                        Ok(c) => c,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            if let Some(mut cap) = capture.take() {
                                cap.stop();
                            }
                            return;
                        }
                    };
                    match cmd {
                        WizardCmd::Start { device_hint, sensitivity: s } => {
                            sensitivity = s;
                            // Opening the wizard again starts a fresh recording session and
                            // retires the old trigger word right away.
                            if let Ok(p) = Config::get_voice_templates_path() {
                                if std::fs::remove_file(&p).is_ok() {
                                    let _ = crate::ipc::send(crate::ipc::Command::ReloadConfig);
                                }
                            }
                            takes = [None, None, None, None, None, None];
                            current_take = 0;
                            testing = false;
                            cached_templates = None;
                            if let Some(mut cap) = capture.take() {
                                cap.stop();
                            }
                            match WizardCapture::start(&device_hint) {
                                Ok(cap) => {
                                    let _ = event_tx.send(WizardEvent::CaptureReady);
                                    capture = Some(cap);
                                }
                                Err(e) => {
                                    let _ = event_tx.send(WizardEvent::CaptureError(e.to_string()));
                                }
                            }
                        }
                        WizardCmd::RedoTake(idx) => {
                            if idx < 6 {
                                takes[idx] = None;
                                current_take = idx;
                            }
                        }
                        WizardCmd::ResetEnrollment => {
                            takes = [None, None, None, None, None, None];
                            current_take = 0;
                            testing = false;
                        }
                        WizardCmd::UpdateSensitivity(s) => {
                            sensitivity = s;
                        }
                        WizardCmd::Stop => {
                            if let Some(mut cap) = capture.take() {
                                cap.stop();
                            }
                            level_worker.store(0, Ordering::Relaxed);
                        }
                    }
                }

                // If active capture, mirror level and process utterances
                if let Some(ref cap) = capture {
                    level_worker.store(cap.current_level().to_bits(), Ordering::Relaxed);

                    while let Some(audio) = cap.try_recv_utterance() {
                        if !testing {
                            let mfcc = extract_mfcc(&audio);
                            takes[current_take] = Some(mfcc);

                            if takes.iter().all(|t| t.is_some()) {
                                let all_takes: Vec<Vec<Vec<f32>>> = takes
                                    .iter()
                                    .filter_map(|t| t.clone())
                                    .collect();
                                match enroll(&all_takes) {
                                    Ok(templates) => {
                                        if let Ok(p) = Config::get_voice_templates_path() {
                                            match templates.save(&p) {
                                                Ok(()) => {
                                                    let _ = crate::ipc::send(crate::ipc::Command::ReloadConfig);
                                                }
                                                Err(e) => eprintln!("Lapse: could not save voice templates: {}", e),
                                            }
                                        }
                                        cached_templates = Some(templates);
                                        testing = true;
                                        let _ = event_tx.send(WizardEvent::EnrollmentSuccess);
                                    }
                                    Err(EnrollmentError::Outlier { take_index, .. }) => {
                                        takes[take_index] = None;
                                        current_take = take_index;
                                        let _ = event_tx.send(WizardEvent::EnrollmentOutlier { take_index });
                                    }
                                    Err(e) => {
                                        let _ = event_tx.send(WizardEvent::EnrollmentFailed(e.to_string()));
                                    }
                                }
                            } else {
                                let recorded_idx = current_take;
                                if let Some(next_idx) = takes.iter().position(|t| t.is_none()) {
                                    current_take = next_idx;
                                }
                                let _ = event_tx.send(WizardEvent::TakeRecorded {
                                    index: recorded_idx,
                                    next_take: current_take,
                                });
                            }
                        } else {
                            if cached_templates.is_none() {
                                if let Ok(p) = Config::get_voice_templates_path() {
                                    cached_templates = VoiceTemplates::load(&p).ok();
                                }
                            }
                            if let Some(ref tpl) = cached_templates {
                                let mfcc = extract_mfcc(&audio);
                                let res = match_utterance(tpl, &mfcc, sensitivity);
                                let _ = event_tx.send(WizardEvent::TestResult {
                                    matched: res.matched,
                                    distance: res.distance,
                                    threshold: res.threshold,
                                });
                            }
                        }
                    }
                }

                std::thread::sleep(Duration::from_millis(15));
            }
        });

        Self {
            cmd_tx,
            event_rx: Arc::new(std::sync::Mutex::new(event_rx)),
            level,
        }
    }

    pub fn send(&self, cmd: WizardCmd) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn try_recv(&self) -> Option<WizardEvent> {
        self.event_rx.lock().ok()?.try_recv().ok()
    }

    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}
