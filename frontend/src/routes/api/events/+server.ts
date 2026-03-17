import type { RequestHandler } from './$types';

// Mock SSE endpoint that simulates live BACnet point value changes.
// In production, the firmware serves this at /api/events.

const POINTS = [
	{ key: 'analog-input:0', base: 55.2, drift: 1.5 },
	{ key: 'analog-input:1', base: 72.8, drift: 0.8 },
	{ key: 'analog-input:2', base: 63.1, drift: 1.2 },
	{ key: 'analog-input:3', base: 42.5, drift: 2.0 },
	{ key: 'analog-input:4', base: 1.25, drift: 0.15 },
	{ key: 'analog-output:0', base: 45.0, drift: 5.0 },
	{ key: 'analog-output:2', base: 72.0, drift: 3.0 },
	{ key: 'analog-output:3', base: 68.0, drift: 3.0 },
	{ key: 'analog-output:4', base: 35.0, drift: 4.0 },
	{ key: 'analog-value:0', base: 55.0, drift: 0 },
	{ key: 'analog-input:5', base: 73.4, drift: 0.6 },
	{ key: 'analog-input:6', base: 320, drift: 30 },
	{ key: 'analog-output:5', base: 62.0, drift: 4.0 },
	{ key: 'analog-input:7', base: 44.2, drift: 0.5 },
	{ key: 'analog-input:8', base: 56.1, drift: 0.8 },
	{ key: 'analog-input:9', base: 142.7, drift: 8.0 },
];

function randomDrift(base: number, drift: number): number {
	if (drift === 0) return base;
	return Math.round((base + (Math.random() - 0.5) * 2 * drift) * 100) / 100;
}

export const GET: RequestHandler = async () => {
	let closed = false;
	let interval: ReturnType<typeof setInterval>;

	const stream = new ReadableStream({
		start(controller) {
			const encoder = new TextEncoder();

			interval = setInterval(() => {
				if (closed) return;
				try {
					const count = 2 + Math.floor(Math.random() * 3);
					const updates: Record<string, number> = {};
					for (let i = 0; i < count; i++) {
						const pt = POINTS[Math.floor(Math.random() * POINTS.length)];
						updates[pt.key] = randomDrift(pt.base, pt.drift);
					}
					controller.enqueue(encoder.encode(`data: ${JSON.stringify(updates)}\n\n`));
				} catch {
					// Controller closed — clean up
					closed = true;
					clearInterval(interval);
				}
			}, 1000);
		},
		cancel() {
			closed = true;
			clearInterval(interval);
		}
	});

	return new Response(stream, {
		headers: {
			'Content-Type': 'text/event-stream',
			'Cache-Control': 'no-cache',
			'Connection': 'keep-alive',
		}
	});
};
