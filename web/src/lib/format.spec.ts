import { describe, expect, it } from 'vitest';
import { clock, count, duration, percent, signed, stamp } from './format.js';

/**
 * The cases are `src/fmt.rs`'s own tests, transcribed. If a duration ever
 * renders one way in the report and another in the app, that is two
 * measurements of one session, and this is what catches it.
 */
describe('format', () => {
	it('reads durations at the right scale', () => {
		expect(duration(0)).toBe('0s');
		expect(duration(59)).toBe('59s');
		expect(duration(60)).toBe('1m');
		expect(duration(3599)).toBe('59m');
		expect(duration(3600)).toBe('1h00m');
		expect(duration(11100)).toBe('3h05m');
		expect(duration(86399)).toBe('23h59m');
		expect(duration(86400)).toBe('1d00h');
		// The corpus's worst case: 54 days, not 1297 hours.
		expect(duration(4669200)).toBe('54d01h');
	});

	it('groups counts in threes', () => {
		expect(count(0)).toBe('0');
		expect(count(999)).toBe('999');
		expect(count(1000)).toBe('1,000');
		expect(count(1234567)).toBe('1,234,567');
	});

	it('keeps two decimals on a percentage', () => {
		expect(percent(45 / 2777)).toBe('1.62%');
		expect(percent(0.25)).toBe('25.00%');
		expect(percent(1 / 338)).toBe('0.30%');
		expect(percent(1)).toBe('100.00%');
	});

	it('signs a delta', () => {
		expect(signed(9)).toBe('+9');
		expect(signed(0)).toBe('+0');
		expect(signed(-2)).toBe('−2');
	});

	it('stamps in UTC, and says nothing rather than guessing', () => {
		expect(stamp('2026-08-20T10:00:00Z')).toBe('2026-08-20 10:00 UTC');
		expect(clock('2026-08-20T10:05:30Z')).toBe('10:05');
		expect(stamp(undefined)).toBe('—');
		expect(clock('not a time')).toBe('—');
	});
});
