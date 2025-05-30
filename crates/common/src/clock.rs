// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Real-time and static test `Clock` implementations.

use std::{
    collections::{BTreeMap, BinaryHeap, HashMap},
    fmt::Debug,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use chrono::{DateTime, Utc};
use futures::Stream;
use nautilus_core::{
    AtomicTime, UnixNanos,
    correctness::{check_positive_u64, check_predicate_true, check_valid_string},
    time::get_atomic_clock_realtime,
};
use tokio::sync::Mutex;
use ustr::Ustr;

use crate::timer::{
    LiveTimer, TestTimer, TimeEvent, TimeEventCallback, TimeEventHandlerV2, create_valid_interval,
};

/// Represents a type of clock.
///
/// # Notes
///
/// An active timer is one which has not expired (`timer.is_expired == False`).
pub trait Clock: Debug {
    /// Returns the current date and time as a timezone-aware `DateTime<UTC>`.
    fn utc_now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(self.timestamp_ns().as_i64())
    }

    /// Returns the current UNIX timestamp in nanoseconds (ns).
    fn timestamp_ns(&self) -> UnixNanos;

    /// Returns the current UNIX timestamp in microseconds (μs).
    fn timestamp_us(&self) -> u64;

    /// Returns the current UNIX timestamp in milliseconds (ms).
    fn timestamp_ms(&self) -> u64;

    /// Returns the current UNIX timestamp in seconds.
    fn timestamp(&self) -> f64;

    /// Returns the names of active timers in the clock.
    fn timer_names(&self) -> Vec<&str>;

    /// Returns the count of active timers in the clock.
    fn timer_count(&self) -> usize;

    /// Register a default event handler for the clock. If a `Timer`
    /// does not have an event handler, then this handler is used.
    fn register_default_handler(&mut self, callback: TimeEventCallback);

    /// Get handler for [`TimeEvent`]
    ///
    /// Note: Panics if the event does not have an associated handler
    fn get_handler(&self, event: TimeEvent) -> TimeEventHandlerV2;

    /// Set a `Timer` to alert at a particular time. Optional
    /// callback gets used to handle generated events.
    /// # Errors
    ///
    /// Returns an error if `name` is invalid, `alert_time_ns` is non-positive when not allowed,
    /// or any predicate check fails.
    fn set_time_alert_ns(
        &mut self,
        name: &str,
        alert_time_ns: UnixNanos,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()>;

    /// Set a `Timer` to start alerting at every interval
    /// between start and stop time. Optional callback gets
    /// used to handle generated event.
    /// # Errors
    ///
    /// Returns an error if `name` is invalid, `interval_ns` is not positive,
    /// or if any predicate check fails.
    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()>;

    /// Returns the time interval in which the timer `name` is triggered.
    ///
    /// If the timer doesn't exist `None` is returned.
    fn next_time_ns(&self, name: &str) -> Option<UnixNanos>;

    /// Cancels the timer with `name`.
    fn cancel_timer(&mut self, name: &str);

    /// Cancels all timers.
    fn cancel_timers(&mut self);

    /// Resets the clock by clearing it's internal state.
    fn reset(&mut self);
}

/// A static test clock.
///
/// Stores the current timestamp internally which can be advanced.
#[derive(Debug)]
pub struct TestClock {
    time: AtomicTime,
    // Use btree map to ensure stable ordering when scanning for timers in `advance_time`
    timers: BTreeMap<Ustr, TestTimer>,
    default_callback: Option<TimeEventCallback>,
    callbacks: HashMap<Ustr, TimeEventCallback>,
    heap: BinaryHeap<TimeEvent>,
}

impl TestClock {
    /// Creates a new [`TestClock`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: AtomicTime::new(false, UnixNanos::default()),
            timers: BTreeMap::new(),
            default_callback: None,
            callbacks: HashMap::new(),
            heap: BinaryHeap::new(),
        }
    }

    /// Returns a reference to the internal timers for the clock.
    #[must_use]
    pub const fn get_timers(&self) -> &BTreeMap<Ustr, TestTimer> {
        &self.timers
    }

    /// Advances the internal clock to the specified `to_time_ns` and optionally sets the clock to that time.
    ///
    /// This function ensures that the clock behaves in a non-decreasing manner. If `set_time` is `true`,
    /// the internal clock will be updated to the value of `to_time_ns`. Otherwise, the clock will advance
    /// without explicitly setting the time.
    ///
    /// The method processes active timers, advancing them to `to_time_ns`, and collects any `TimeEvent`
    /// objects that are triggered as a result. Only timers that are not expired are processed.
    ///
    /// # Panics
    ///
    /// Panics if `to_time_ns` is less than the current internal clock time.
    pub fn advance_time(&mut self, to_time_ns: UnixNanos, set_time: bool) -> Vec<TimeEvent> {
        // Time should be non-decreasing
        assert!(
            to_time_ns >= self.time.get_time_ns(),
            "`to_time_ns` {to_time_ns} was < `self.time.get_time_ns()` {}",
            self.time.get_time_ns()
        );

        if set_time {
            self.time.set_time(to_time_ns);
        }

        // Iterate and advance timers and collect events. Only retain alive timers.
        let mut events: Vec<TimeEvent> = Vec::new();
        self.timers.retain(|_, timer| {
            timer.advance(to_time_ns).for_each(|event| {
                events.push(event);
            });

            !timer.is_expired()
        });

        events.sort_by(|a, b| a.ts_event.cmp(&b.ts_event));
        events
    }

    /// Advances the internal clock to the specified `to_time_ns` and optionally sets the clock to that time.
    ///
    /// Pushes the [`TimeEvent`]s on the heap to ensure ordering
    ///
    /// Note: `set_time` is not used but present to keep backward compatible api call
    ///
    /// # Panics
    ///
    /// Panics if `to_time_ns` is less than the current internal clock time.
    pub fn advance_to_time_on_heap(&mut self, to_time_ns: UnixNanos) {
        // Time should be non-decreasing
        assert!(
            to_time_ns >= self.time.get_time_ns(),
            "`to_time_ns` {to_time_ns} was < `self.time.get_time_ns()` {}",
            self.time.get_time_ns()
        );

        self.time.set_time(to_time_ns);

        // Iterate and advance timers and push events to heap. Only retain alive timers.
        self.timers.retain(|_, timer| {
            timer.advance(to_time_ns).for_each(|event| {
                self.heap.push(event);
            });

            !timer.is_expired()
        });
    }

    /// Matches `TimeEvent` objects with their corresponding event handlers.
    ///
    /// This function takes an `events` vector of `TimeEvent` objects, assumes they are already sorted
    /// by their `ts_event`, and matches them with the appropriate callback handler from the internal
    /// registry of callbacks. If no specific callback is found for an event, the default callback is used.
    ///
    /// # Panics
    ///
    /// Panics if the default callback is not set for the clock when matching handlers.
    #[must_use]
    pub fn match_handlers(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandlerV2> {
        events
            .into_iter()
            .map(|event| {
                let callback = self.callbacks.get(&event.name).cloned().unwrap_or_else(|| {
                    // If callback_py is None, use the default_callback_py
                    // TODO: clone for now
                    self.default_callback
                        .clone()
                        .expect("Default callback should exist")
                });
                TimeEventHandlerV2::new(event, callback)
            })
            .collect()
    }
}

impl Iterator for TestClock {
    type Item = TimeEventHandlerV2;

    fn next(&mut self) -> Option<Self::Item> {
        self.heap.pop().map(|event| self.get_handler(event))
    }
}

impl Default for TestClock {
    /// Creates a new default [`TestClock`] instance.
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for TestClock {
    type Target = AtomicTime;

    fn deref(&self) -> &Self::Target {
        &self.time
    }
}

impl Clock for TestClock {
    fn timestamp_ns(&self) -> UnixNanos {
        self.time.get_time_ns()
    }

    fn timestamp_us(&self) -> u64 {
        self.time.get_time_us()
    }

    fn timestamp_ms(&self) -> u64 {
        self.time.get_time_ms()
    }

    fn timestamp(&self) -> f64 {
        self.time.get_time()
    }

    fn timer_names(&self) -> Vec<&str> {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    fn timer_count(&self) -> usize {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired())
            .count()
    }

    fn register_default_handler(&mut self, callback: TimeEventCallback) {
        self.default_callback = Some(callback);
    }

    /// Returns the handler for the given `TimeEvent`.
    ///
    /// # Panics
    ///
    /// Panics if no event-specific or default callback has been registered for the event.
    fn get_handler(&self, event: TimeEvent) -> TimeEventHandlerV2 {
        // Get the callback from either the event-specific callbacks or default callback
        let callback = self
            .callbacks
            .get(&event.name)
            .cloned()
            .or_else(|| self.default_callback.clone())
            .unwrap_or_else(|| panic!("Event '{}' should have associated handler", event.name));

        TimeEventHandlerV2::new(event, callback)
    }

    fn set_time_alert_ns(
        &mut self,
        name: &str,
        mut alert_time_ns: UnixNanos, // mut allows adjustment based on allow_past
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        check_valid_string(name, stringify!(name))?;

        let name = Ustr::from(name);
        let allow_past = allow_past.unwrap_or(true);

        check_predicate_true(
            callback.is_some()
                | self.callbacks.contains_key(&name)
                | self.default_callback.is_some(),
            "No callbacks provided",
        )?;

        match callback {
            Some(callback_py) => self.callbacks.insert(name, callback_py),
            None => None,
        };

        // This allows to reuse a time alert without updating the callback, for example for non regular monthly alerts
        self.cancel_timer(name.as_str());

        let ts_now = self.get_time_ns();

        if alert_time_ns < ts_now {
            if allow_past {
                alert_time_ns = ts_now;
                log::warn!(
                    "Timer '{name}' alert time {} was in the past, adjusted to current time for immediate firing",
                    alert_time_ns.to_rfc3339(),
                );
            } else {
                anyhow::bail!(
                    "Timer '{name}' alert time {} was in the past (current time is {})",
                    alert_time_ns.to_rfc3339(),
                    ts_now.to_rfc3339(),
                );
            }
        }

        // Safe to calculate interval now that we've ensured alert_time_ns >= ts_now
        let interval_ns = create_valid_interval((alert_time_ns - ts_now).into());

        let timer = TestTimer::new(name, interval_ns, ts_now, Some(alert_time_ns));
        self.timers.insert(name, timer);

        Ok(())
    }

    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        check_valid_string(name, stringify!(name))?;
        check_positive_u64(interval_ns, stringify!(interval_ns))?;
        check_predicate_true(
            callback.is_some() | self.default_callback.is_some(),
            "No callbacks provided",
        )?;

        let name = Ustr::from(name);
        let allow_past = allow_past.unwrap_or(true);

        match callback {
            Some(callback_py) => self.callbacks.insert(name, callback_py),
            None => None,
        };

        let mut start_time_ns = start_time_ns;
        let ts_now = self.get_time_ns();

        if start_time_ns == 0 {
            // Zero start time indicates no explicit start; we use the current time
            start_time_ns = self.timestamp_ns();
        } else if start_time_ns < ts_now && !allow_past {
            anyhow::bail!(
                "Timer '{name}' start time {} was in the past (current time is {})",
                start_time_ns.to_rfc3339(),
                ts_now.to_rfc3339(),
            );
        }

        if let Some(stop_time) = stop_time_ns {
            if stop_time <= start_time_ns {
                anyhow::bail!(
                    "Timer '{name}' stop time {} must be after start time {}",
                    stop_time.to_rfc3339(),
                    start_time_ns.to_rfc3339(),
                );
            }
        }

        let interval_ns = create_valid_interval(interval_ns);

        let timer = TestTimer::new(name, interval_ns, start_time_ns, stop_time_ns);
        self.timers.insert(name, timer);

        Ok(())
    }

    fn next_time_ns(&self, name: &str) -> Option<UnixNanos> {
        self.timers
            .get(&Ustr::from(name))
            .map(|timer| timer.next_time_ns())
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(&Ustr::from(name));
        if let Some(mut timer) = timer {
            timer.cancel();
        }
    }

    fn cancel_timers(&mut self) {
        for timer in &mut self.timers.values_mut() {
            timer.cancel();
        }

        self.timers.clear();
    }

    fn reset(&mut self) {
        self.time = AtomicTime::new(false, UnixNanos::default());
        self.timers = BTreeMap::new();
        self.heap = BinaryHeap::new();
        self.callbacks = HashMap::new();
    }
}

/// A real-time clock which uses system time.
///
/// Timestamps are guaranteed to be unique and monotonically increasing.
#[derive(Debug)]
pub struct LiveClock {
    time: &'static AtomicTime,
    timers: HashMap<Ustr, LiveTimer>,
    default_callback: Option<TimeEventCallback>,
    pub heap: Arc<Mutex<BinaryHeap<TimeEvent>>>,
    #[allow(dead_code)]
    callbacks: HashMap<Ustr, TimeEventCallback>,
}

impl LiveClock {
    /// Creates a new [`LiveClock`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: get_atomic_clock_realtime(),
            timers: HashMap::new(),
            default_callback: None,
            heap: Arc::new(Mutex::new(BinaryHeap::new())),
            callbacks: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get_event_stream(&self) -> TimeEventStream {
        TimeEventStream::new(self.heap.clone())
    }

    #[must_use]
    pub const fn get_timers(&self) -> &HashMap<Ustr, LiveTimer> {
        &self.timers
    }

    // Clean up expired timers. Retain only live ones
    fn clear_expired_timers(&mut self) {
        self.timers.retain(|_, timer| !timer.is_expired());
    }
}

impl Default for LiveClock {
    /// Creates a new default [`LiveClock`] instance.
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for LiveClock {
    type Target = AtomicTime;

    fn deref(&self) -> &Self::Target {
        self.time
    }
}

impl Clock for LiveClock {
    fn timestamp_ns(&self) -> UnixNanos {
        self.time.get_time_ns()
    }

    fn timestamp_us(&self) -> u64 {
        self.time.get_time_us()
    }

    fn timestamp_ms(&self) -> u64 {
        self.time.get_time_ms()
    }

    fn timestamp(&self) -> f64 {
        self.time.get_time()
    }

    fn timer_names(&self) -> Vec<&str> {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    fn timer_count(&self) -> usize {
        self.timers
            .iter()
            .filter(|(_, timer)| !timer.is_expired())
            .count()
    }

    fn register_default_handler(&mut self, handler: TimeEventCallback) {
        self.default_callback = Some(handler);
    }

    /// # Panics
    ///
    /// This function panics if:
    /// - The `clock_v2` feature is not enabled.
    /// - The event does not have an associated handler (see trait documentation).
    #[allow(unused_variables)]
    fn get_handler(&self, event: TimeEvent) -> TimeEventHandlerV2 {
        #[cfg(not(feature = "clock_v2"))]
        panic!("Cannot get live clock handler without 'clock_v2' feature");

        // Get the callback from either the event-specific callbacks or default callback
        #[cfg(feature = "clock_v2")]
        {
            let callback = self
                .callbacks
                .get(&event.name)
                .cloned()
                .or_else(|| self.default_callback.clone())
                .unwrap_or_else(|| panic!("Event '{}' should have associated handler", event.name));

            TimeEventHandlerV2::new(event, callback)
        }
    }

    fn set_time_alert_ns(
        &mut self,
        name: &str,
        mut alert_time_ns: UnixNanos, // mut allows adjustment based on allow_past
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        check_valid_string(name, stringify!(name))?;

        let name = Ustr::from(name);
        let allow_past = allow_past.unwrap_or(true);

        check_predicate_true(
            callback.is_some()
                | self.callbacks.contains_key(&name)
                | self.default_callback.is_some(),
            "No callbacks provided",
        )?;

        #[cfg(feature = "clock_v2")]
        {
            match callback.clone() {
                Some(callback) => self.callbacks.insert(name, callback),
                None => None,
            };
        }

        let callback = match callback {
            Some(callback) => callback,
            None => {
                if self.callbacks.contains_key(&name) {
                    self.callbacks.get(&name).unwrap().clone()
                } else {
                    self.default_callback.clone().unwrap()
                }
            }
        };

        // This allows to reuse a time alert without updating the callback, for example for non regular monthly alerts
        self.cancel_timer(name.as_str());

        let ts_now = self.get_time_ns();

        // Handle past timestamps based on flag
        if alert_time_ns < ts_now {
            if allow_past {
                alert_time_ns = ts_now;
                log::warn!(
                    "Timer '{name}' alert time {} was in the past, adjusted to current time for immediate firing",
                    alert_time_ns.to_rfc3339(),
                );
            } else {
                anyhow::bail!(
                    "Timer '{name}' alert time {} was in the past (current time is {})",
                    alert_time_ns.to_rfc3339(),
                    ts_now.to_rfc3339(),
                );
            }
        }

        // Safe to calculate interval now that we've ensured alert_time_ns >= ts_now
        let interval_ns = create_valid_interval((alert_time_ns - ts_now).into());

        #[cfg(not(feature = "clock_v2"))]
        let mut timer = LiveTimer::new(name, interval_ns, ts_now, Some(alert_time_ns), callback);

        #[cfg(feature = "clock_v2")]
        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            ts_now,
            Some(alert_time_ns),
            callback,
            self.heap.clone(),
        );

        timer.start();

        self.clear_expired_timers();
        self.timers.insert(name, timer);

        Ok(())
    }

    fn set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: UnixNanos,
        stop_time_ns: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        check_valid_string(name, stringify!(name))?;
        check_positive_u64(interval_ns, stringify!(interval_ns))?;
        check_predicate_true(
            callback.is_some() | self.default_callback.is_some(),
            "No callbacks provided",
        )?;

        let name = Ustr::from(name);
        let allow_past = allow_past.unwrap_or(true);

        let callback = match callback {
            Some(callback) => callback,
            None => self.default_callback.clone().unwrap(),
        };

        #[cfg(feature = "clock_v2")]
        {
            self.callbacks.insert(name, callback.clone());
        }

        let mut start_time_ns = start_time_ns;
        let ts_now = self.get_time_ns();

        if start_time_ns == 0 {
            // Zero start time indicates no explicit start; we use the current time
            start_time_ns = self.timestamp_ns();
        } else if start_time_ns < ts_now && !allow_past {
            anyhow::bail!(
                "Timer '{name}' start time {} was in the past (current time is {})",
                start_time_ns.to_rfc3339(),
                ts_now.to_rfc3339(),
            );
        }

        if let Some(stop_time) = stop_time_ns {
            if stop_time <= start_time_ns {
                anyhow::bail!(
                    "Timer '{name}' stop time {} must be after start time {}",
                    stop_time.to_rfc3339(),
                    start_time_ns.to_rfc3339(),
                );
            }
        }

        let interval_ns = create_valid_interval(interval_ns);

        #[cfg(not(feature = "clock_v2"))]
        let mut timer = LiveTimer::new(name, interval_ns, start_time_ns, stop_time_ns, callback);

        #[cfg(feature = "clock_v2")]
        let mut timer = LiveTimer::new(
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            callback,
            self.heap.clone(),
        );
        timer.start();

        self.clear_expired_timers();
        self.timers.insert(name, timer);

        Ok(())
    }

    fn next_time_ns(&self, name: &str) -> Option<UnixNanos> {
        self.timers
            .get(&Ustr::from(name))
            .map(|timer| timer.next_time_ns())
    }

    fn cancel_timer(&mut self, name: &str) {
        let timer = self.timers.remove(&Ustr::from(name));
        if let Some(mut timer) = timer {
            timer.cancel();
        }
    }

    fn cancel_timers(&mut self) {
        for timer in &mut self.timers.values_mut() {
            timer.cancel();
        }

        self.timers.clear();
    }

    fn reset(&mut self) {
        self.timers = HashMap::new();
        self.heap = Arc::new(Mutex::new(BinaryHeap::new()));
        self.callbacks = HashMap::new();
    }
}

// Helper struct to stream events from the heap
#[derive(Debug)]
pub struct TimeEventStream {
    heap: Arc<Mutex<BinaryHeap<TimeEvent>>>,
}

impl TimeEventStream {
    pub const fn new(heap: Arc<Mutex<BinaryHeap<TimeEvent>>>) -> Self {
        Self { heap }
    }
}

impl Stream for TimeEventStream {
    type Item = TimeEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut heap = match self.heap.try_lock() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::error!("Unable to get LiveClock heap lock: {e}");
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };

        if let Some(event) = heap.pop() {
            Poll::Ready(Some(event))
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use rstest::{fixture, rstest};

    use super::*;

    #[derive(Default)]
    struct TestCallback {
        called: Rc<RefCell<bool>>,
    }

    impl TestCallback {
        const fn new(called: Rc<RefCell<bool>>) -> Self {
            Self { called }
        }
    }

    impl From<TestCallback> for TimeEventCallback {
        fn from(callback: TestCallback) -> Self {
            Self::Rust(Rc::new(move |_event: TimeEvent| {
                *callback.called.borrow_mut() = true;
            }))
        }
    }

    #[fixture]
    pub fn test_clock() -> TestClock {
        let mut clock = TestClock::new();
        clock.register_default_handler(TestCallback::default().into());
        clock
    }

    #[rstest]
    fn test_time_monotonicity(mut test_clock: TestClock) {
        let initial_time = test_clock.timestamp_ns();
        test_clock.advance_time((*initial_time + 1000).into(), true);
        assert!(test_clock.timestamp_ns() > initial_time);
    }

    #[rstest]
    fn test_timer_registration(mut test_clock: TestClock) {
        test_clock
            .set_time_alert_ns(
                "test_timer",
                (*test_clock.timestamp_ns() + 1000).into(),
                None,
                None,
            )
            .unwrap();
        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(test_clock.timer_names(), vec!["test_timer"]);
    }

    #[rstest]
    fn test_timer_expiration(mut test_clock: TestClock) {
        let alert_time = (*test_clock.timestamp_ns() + 1000).into();
        test_clock
            .set_time_alert_ns("test_timer", alert_time, None, None)
            .unwrap();
        let events = test_clock.advance_time(alert_time, true);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name.as_str(), "test_timer");
    }

    #[rstest]
    fn test_timer_cancellation(mut test_clock: TestClock) {
        test_clock
            .set_time_alert_ns(
                "test_timer",
                (*test_clock.timestamp_ns() + 1000).into(),
                None,
                None,
            )
            .unwrap();
        assert_eq!(test_clock.timer_count(), 1);
        test_clock.cancel_timer("test_timer");
        assert_eq!(test_clock.timer_count(), 0);
    }

    #[rstest]
    fn test_time_advancement(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        test_clock
            .set_timer_ns("test_timer", 1000, start_time, None, None, None)
            .unwrap();
        let events = test_clock.advance_time((*start_time + 2500).into(), true);
        assert_eq!(events.len(), 2);
        assert_eq!(*events[0].ts_event, *start_time + 1000);
        assert_eq!(*events[1].ts_event, *start_time + 2000);
    }

    #[rstest]
    fn test_default_and_custom_callbacks() {
        let mut clock = TestClock::new();
        let default_called = Rc::new(RefCell::new(false));
        let custom_called = Rc::new(RefCell::new(false));

        let default_callback = TestCallback::new(Rc::clone(&default_called));
        let custom_callback = TestCallback::new(Rc::clone(&custom_called));

        clock.register_default_handler(TimeEventCallback::from(default_callback));
        clock
            .set_time_alert_ns(
                "default_timer",
                (*clock.timestamp_ns() + 1000).into(),
                None,
                None,
            )
            .unwrap();
        clock
            .set_time_alert_ns(
                "custom_timer",
                (*clock.timestamp_ns() + 1000).into(),
                Some(TimeEventCallback::from(custom_callback)),
                None,
            )
            .unwrap();

        let events = clock.advance_time((*clock.timestamp_ns() + 1000).into(), true);
        let handlers = clock.match_handlers(events);

        for handler in handlers {
            handler.callback.call(handler.event);
        }

        assert!(*default_called.borrow());
        assert!(*custom_called.borrow());
    }

    #[rstest]
    fn test_multiple_timers(mut test_clock: TestClock) {
        let start_time = test_clock.timestamp_ns();
        test_clock
            .set_timer_ns("timer1", 1000, start_time, None, None, None)
            .unwrap();
        test_clock
            .set_timer_ns("timer2", 2000, start_time, None, None, None)
            .unwrap();
        let events = test_clock.advance_time((*start_time + 2000).into(), true);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].name.as_str(), "timer1");
        assert_eq!(events[1].name.as_str(), "timer1");
        assert_eq!(events[2].name.as_str(), "timer2");
    }

    #[rstest]
    fn test_allow_past_parameter_true(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(2000));
        let current_time = test_clock.timestamp_ns();
        let past_time = UnixNanos::from(current_time.as_u64() - 1000);

        // With allow_past=true (default), should adjust to current time and succeed
        test_clock
            .set_time_alert_ns("past_timer", past_time, None, Some(true))
            .unwrap();

        // Verify timer was created with adjusted time
        assert_eq!(test_clock.timer_count(), 1);
        assert_eq!(test_clock.timer_names(), vec!["past_timer"]);

        // Next time should be at or after current time, not in the past
        let next_time = test_clock.next_time_ns("past_timer").unwrap();
        assert!(next_time >= current_time);
    }

    #[rstest]
    fn test_allow_past_parameter_false(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(2000));
        let current_time = test_clock.timestamp_ns();
        let past_time = current_time - 1000;

        // With allow_past=false, should fail for past times
        let result = test_clock.set_time_alert_ns("past_timer", past_time, None, Some(false));

        // Verify the operation failed with appropriate error
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("was in the past"));

        // Verify no timer was created
        assert_eq!(test_clock.timer_count(), 0);
        assert!(test_clock.timer_names().is_empty());
    }

    #[rstest]
    fn test_invalid_stop_time_validation(mut test_clock: TestClock) {
        test_clock.set_time(UnixNanos::from(2000));
        let current_time = test_clock.timestamp_ns();
        let start_time = current_time + 1000;
        let stop_time = current_time + 500; // Stop time before start time

        // Should fail because stop_time < start_time
        let result = test_clock.set_timer_ns(
            "invalid_timer",
            100,
            start_time,
            Some(stop_time),
            None,
            None,
        );

        // Verify the operation failed with appropriate error
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("must be after start time"));

        // Verify no timer was created
        assert_eq!(test_clock.timer_count(), 0);
    }
}
