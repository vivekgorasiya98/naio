use crate::handler::HandlerFn;
use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Job {
    pub name: String,
    pub payload: String,
    pub attempts: u32,
}

pub struct JobQueue {
    pending: Mutex<VecDeque<Job>>,
    handlers: DashMap<String, HandlerFn>,
}

impl JobQueue {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(VecDeque::new()),
            handlers: DashMap::new(),
        }
    }

    pub fn enqueue(&self, name: impl Into<String>, payload: impl Into<String>) -> Result<(), String> {
        self.pending.lock().unwrap().push_back(Job {
            name: name.into(),
            payload: payload.into(),
            attempts: 0,
        });
        Ok(())
    }

    pub fn register_handler(&self, name: impl Into<String>, handler: HandlerFn) {
        self.handlers.insert(name.into(), handler);
    }

    pub fn pop(&self) -> Option<Job> {
        self.pending.lock().unwrap().pop_front()
    }

    pub fn requeue(&self, mut job: Job) {
        job.attempts += 1;
        if job.attempts < 5 {
            self.pending.lock().unwrap().push_back(job);
        }
    }

    pub fn handler(&self, name: &str) -> Option<HandlerFn> {
        self.handlers.get(name).map(|h| h.clone())
    }

    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }
}

pub type SharedJobQueue = Arc<JobQueue>;

pub struct CronJob {
    pub schedule: String,
    pub handler: HandlerFn,
}

pub struct CronScheduler {
    jobs: Mutex<Vec<CronJob>>,
}

impl CronScheduler {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(Vec::new()),
        }
    }

    pub fn register(&self, schedule: impl Into<String>, handler: HandlerFn) -> Result<(), String> {
        let sched = schedule.into();
        if sched.split_whitespace().count() < 5 {
            return Err("invalid cron expression (E2201)".into());
        }
        self.jobs.lock().unwrap().push(CronJob {
            schedule: sched,
            handler,
        });
        Ok(())
    }

    pub fn jobs(&self) -> Vec<(String, HandlerFn)> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .map(|j| (j.schedule.clone(), j.handler.clone()))
            .collect()
    }
}

pub type SharedCronScheduler = Arc<CronScheduler>;
