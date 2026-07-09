# Time standard library

Wall-clock timestamps, formatting, parsing, IANA time zones, and date arithmetic for Niao programs. Implemented in Rust via `chrono` and `chrono-tz`.

> **Note:** Core builtins `now_ms()` and `now_us()` measure **monotonic** elapsed time since process start (good for benchmarks). The `time` library provides **wall-clock** Unix timestamps.

## Import

```niao
import "time"
```

Use the **`time`** namespace object for short names:

```niao
let now = time.now_unix_ms()
print(time.format(now, "%Y-%m-%d %H:%M:%S", "UTC"))
```

### Custom alias

```niao
import "time" as t

let dt = t.now("local")
print(dt.year)
print(dt.weekday_name)
```

Paths `import "std/time"` and `import "time"` are equivalent.

## Namespace API

### Wall clock

| Method | Description |
|--------|-------------|
| `time.now_unix_ms()` | Unix epoch milliseconds (wall clock) |
| `time.now_unix_s()` | Unix epoch seconds |
| `time.now_iso()` | RFC 3339 string in UTC |
| `time.now_iso_local()` | RFC 3339 string in local timezone |
| `time.now(tz?)` | Datetime object (`tz` defaults to `"local"`) |

### Format & parse

| Method | Description |
|--------|-------------|
| `time.format(ms, fmt, tz?)` | Format timestamp (`tz` defaults to `"UTC"`) |
| `time.parse(text, fmt, tz?)` | Parse string → unix ms (`tz` defaults to `"UTC"`) |
| `time.to_iso(ms, tz?)` | ISO 8601 with offset |

Format strings follow [chrono strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) conventions (`%Y`, `%m`, `%d`, `%H`, `%M`, `%S`, etc.).

### Datetime object

`time.now()` and `time.decompose(ms, tz?)` return an object:

| Field | Type | Description |
|-------|------|-------------|
| `year` | int | Calendar year |
| `month` | int | 1–12 |
| `day` | int | 1–31 |
| `hour` | int | 0–23 |
| `minute` | int | 0–59 |
| `second` | int | 0–59 |
| `millisecond` | int | 0–999 |
| `weekday` | int | 0 = Monday … 6 = Sunday |
| `weekday_name` | string | e.g. `"Friday"` |
| `unix_ms` | int | Original timestamp |
| `timezone` | string | e.g. `"UTC"`, `"local"`, `"Asia/Kolkata"` |
| `utc_offset_ms` | int | UTC offset in milliseconds |

### Construction & decomposition

| Method | Description |
|--------|-------------|
| `time.from_parts(year, month, day, hour?, minute?, second?, ms?, tz?)` | Build unix ms from parts (`tz` defaults to `"local"`) |
| `time.decompose(ms, tz?)` | Split timestamp into datetime object |

### Arithmetic

| Method | Description |
|--------|-------------|
| `time.add_ms(ms, delta)` | Add milliseconds |
| `time.add_seconds(ms, delta)` | Add seconds |
| `time.add_minutes(ms, delta)` | Add minutes |
| `time.add_hours(ms, delta)` | Add hours |
| `time.add_days(ms, delta)` | Add days |
| `time.diff_ms(a, b)` | Returns `a - b` in milliseconds |

### Time zones

| Method | Description |
|--------|-------------|
| `time.utc_offset_ms(tz, ms?)` | UTC offset in ms at `ms` (default: now) |
| `time.list_timezones()` | Sorted list of IANA timezone names |

Timezone strings: `"UTC"`, `"GMT"`, `"local"`, or any IANA name (e.g. `"America/New_York"`, `"Asia/Tokyo"`).

### Calendar helpers

| Method | Description |
|--------|-------------|
| `time.is_leap_year(year)` | Leap year check |
| `time.days_in_month(year, month)` | Days in month (1–12) |
| `time.is_valid_date(year, month, day)` | Valid calendar date |
| `time.start_of_day(ms, tz?)` | Midnight in timezone |
| `time.end_of_day(ms, tz?)` | 23:59:59.999 in timezone |

### Components

| Method | Description |
|--------|-------------|
| `time.year(ms, tz?)` | Year component |
| `time.month(ms, tz?)` | Month component |
| `time.day(ms, tz?)` | Day component |
| `time.hour(ms, tz?)` | Hour component |
| `time.minute(ms, tz?)` | Minute component |
| `time.second(ms, tz?)` | Second component |
| `time.weekday(ms, tz?)` | Weekday 0–6 (Mon–Sun) |
| `time.weekday_name(ms, tz?)` | Weekday name |

### Sleep

| Method | Description |
|--------|-------------|
| `time.sleep_ms(ms)` | Block for `ms` milliseconds |

For threaded programs, `parallel_thread_sleep(ms)` is also available via `import "parallel"`.

## Legacy flat names

Global builtins with `time_` prefix mirror the namespace API:

`time_now_unix_ms`, `time_now_unix_s`, `time_now_iso`, `time_now_iso_local`, `time_now`, `time_format`, `time_parse`, `time_to_iso`, `time_decompose`, `time_from_parts`, `time_add_ms`, `time_add_seconds`, `time_add_minutes`, `time_add_hours`, `time_add_days`, `time_diff_ms`, `time_utc_offset_ms`, `time_is_leap_year`, `time_days_in_month`, `time_is_valid_date`, `time_start_of_day`, `time_end_of_day`, `time_sleep_ms`, `time_list_timezones`, `time_year`, `time_month`, `time_day`, `time_hour`, `time_minute`, `time_second`, `time_weekday`, `time_weekday_name`

## Errors

Parse failures, unknown timezones, and invalid dates return **error values** (not thrown exceptions). Check with `is_error(result)`.

## Examples

```niao
import "time" as t

fn main() {
    let now = t.now_unix_ms()
    print(t.format(now, "%Y-%m-%d %H:%M:%S", "local"))

    let meeting = t.from_parts(2026, 12, 25, 9, 0, 0, 0, "Asia/Kolkata")
    print(t.weekday_name(meeting, "Asia/Kolkata"))

    let reminder = t.add_hours(meeting, -1)
    print(t.to_iso(reminder, "Asia/Kolkata"))
}
```

Run the demo:

```bash
niao run examples/time_demo.niao
```

Run tests:

```bash
niao run tests/time.niao
```
