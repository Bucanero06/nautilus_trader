/* Generated with cbindgen:0.29.0 */

/* Warning, this file is autogenerated by cbindgen. Don't modify this manually. */

#include <stdint.h>
#include <Python.h>

/**
 * Provides a means of accumulating and draining time event handlers.
 */
typedef struct TimeEventAccumulator TimeEventAccumulator;

typedef struct TimeEventAccumulatorAPI {
    struct TimeEventAccumulator *_0;
} TimeEventAccumulatorAPI;

struct TimeEventAccumulatorAPI time_event_accumulator_new(void);

void time_event_accumulator_drop(struct TimeEventAccumulatorAPI accumulator);

void time_event_accumulator_advance_clock(struct TimeEventAccumulatorAPI *accumulator,
                                          TestClock_API *clock,
                                          uint64_t to_time_ns,
                                          uint8_t set_time);

CVec time_event_accumulator_drain(struct TimeEventAccumulatorAPI *accumulator);
