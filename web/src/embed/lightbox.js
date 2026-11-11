// Zoomable lightbox for one image. A separate host element carries its own
// shadow root, so the host page can neither restyle the panel nor be
// affected by it; dialog top-layer placement survives any host overflow or
// z-index value. Keyboard parity: Esc closes (native dialog cancel), +/−
// zoom, 0 resets, arrow keys pan. Touch is supported through pointer
// events: one pointer drags, two-pointer pinch zooms. Reduced-motion
// users get no transitions.

const SCALE_MIN = 1;
const SCALE_MAX = 6;

const STYLES = `
:host { all: initial; display: contents; }
.dialog {
	position: fixed;
	inset: 0;
	width: 100%;
	height: 100%;
	max-width: none;
	max-height: none;
	margin: 0;
	border: 0;
	padding: 0;
	background: rgba(0, 0, 0, .84);
	color: #fff;
	font-family: ui-sans-serif, system-ui, sans-serif;
}
.dialog:not([open]) { display: none; }
.dialog::backdrop { background: transparent; }
.stage {
	position: absolute;
	inset: 0;
	display: grid;
	place-items: center;
	overflow: hidden;
	touch-action: none;
	user-select: none;
	-webkit-user-select: none;
}
img {
	max-width: min(92vw, 1600px);
	max-height: 86vh;
	transition: transform .18s ease;
	transform-origin: center center;
	will-change: transform;
	cursor: grab;
}
img.zoomed { cursor: grabbing; }
@media (prefers-reduced-motion: reduce) {
	img { transition: none; }
}
.controls {
	position: absolute;
	inset-inline-end: 12px;
	inset-block-end: 12px;
	display: flex;
	gap: 8px;
}
.controls button {
	font: inherit;
	color: inherit;
	background: rgba(255, 255, 255, .14);
	border: 1px solid rgba(255, 255, 255, .32);
	border-radius: 999px;
	min-width: 40px;
	min-height: 40px;
	padding: 0 12px;
	cursor: pointer;
}
.controls button:hover { background: rgba(255, 255, 255, .26); }
.caption {
	position: absolute;
	inset-inline: 12px;
	inset-block-start: 12px;
	margin: 0;
	max-width: 60ch;
	color: rgba(255, 255, 255, .92);
	font-size: 14px;
	line-height: 1.4;
	pointer-events: none;
	text-shadow: 0 1px 2px rgba(0, 0, 0, .55);
}
.failed.hidden { display: none; }
.failed a { color: #fff; }
`;

/**
 * Builds one reusable lightbox bound to one window. `disabled` removes all
 * behavior up-front (embed option), keeping the same call surface.
 */
export function createLightbox(window, { disabled = false } = {}) {
	const document = window.document;
	if (disabled) {
		return { open() {}, close() {}, isOpen() { return false; }, dispose() {} };
	}
	const host = document.createElement('div');
	host.dataset.statesuLightbox = '';
	const shadow = host.attachShadow({ mode: 'open' });
	const style = document.createElement('style');
	style.textContent = STYLES;

	const dialog = document.createElement('dialog');
	dialog.className = 'dialog';

	const stage = document.createElement('div');
	stage.className = 'stage';

	const image = document.createElement('img');
	image.decoding = 'async';
	image.alt = '';

	const caption = document.createElement('p');
	caption.className = 'caption';

	const failed = document.createElement('p');
	failed.className = 'failed';
	failed.classList.add('hidden');

	const closeButton = document.createElement('button');
	closeButton.textContent = '\u00d7';
	closeButton.setAttribute('aria-label', 'Close image preview');
	closeButton.className = 'dismiss';
	const controls = document.createElement('div');
	controls.className = 'controls';
	const zoomIn = document.createElement('button');
	zoomIn.textContent = '+';
	zoomIn.setAttribute('aria-label', 'Zoom in');
	const zoomOut = document.createElement('button');
	zoomOut.textContent = '−';
	zoomOut.setAttribute('aria-label', 'Zoom out');
	const reset = document.createElement('button');
	reset.textContent = '1:1';
	reset.setAttribute('aria-label', 'Reset zoom');
	controls.append(zoomIn, zoomOut, reset, closeButton);
	closeButton.addEventListener('click', () => hide());

	stage.append(image);
	dialog.append(stage, failed, caption, controls);
	shadow.append(style, dialog);
	document.body.appendChild(host);
	dialog.setAttribute('aria-label', 'Image preview');

	// Zoom and pan state -----------------------------------------------------
	let scale = 1;
	let offsetX = 0;
	let offsetY = 0;
	let opener = null;

	const applyTransform = () => {
		image.style.transform = `translate(${offsetX}px, ${offsetY}px) scale(${scale})`;
		image.classList.toggle('zoomed', scale > SCALE_MIN + 0.01);
	};

	const setScale = (next, anchorX, anchorY) => {
		const clamped = Math.min(SCALE_MAX, Math.max(SCALE_MIN, next));
		if (clamped === scale) return;
		// Keep the anchored point stable while the view scales.
		const anchorLeft = typeof anchorX === 'number'
			? anchorX - window.innerWidth / 2
			: 0;
		const anchorTop = typeof anchorY === 'number'
			? anchorY - window.innerHeight / 2
			: 0;
		const factor = clamped / scale;
		offsetX = (offsetX - anchorLeft) * factor + anchorLeft;
		offsetY = (offsetY - anchorTop) * factor + anchorTop;
		scale = clamped;
		applyTransform();
	};

	const resetView = () => {
		scale = 1;
		offsetX = 0;
		offsetY = 0;
		applyTransform();
	};

	// Opening and closing ----------------------------------------------------
	/** `fullUrl` is the inspection-grade address; `trigger` regains focus. */
	function open(src, altText, trigger, fullUrl) {
		opener = trigger ?? document.activeElement;
		image.alt = typeof altText === 'string' ? altText : '';
		caption.textContent = image.alt || '';
		failed.classList.add('hidden');
		try {
			if (typeof dialog.showModal === 'function') dialog.showModal();
			else dialog.setAttribute('open', '');
		} catch {
			dialog.setAttribute('open', '');
		}
		resetView();
		image.src = src;
		if (fullUrl && fullUrl !== src) image.dataset.zoomUrl = fullUrl;
		else delete image.dataset.zoomUrl;
	}
	function hide() {
		if (typeof dialog.close === 'function') dialog.close();
		else dialog.removeAttribute('open');
		if (typeof opener?.focus === 'function') opener.focus();
		opener = null;
	}
	const isOpen = () => dialog.hasAttribute('open');

	// Any close (Esc, button, cancel) releases the image bytes and gesture
	// state, so nothing lingers between opens.
	dialog.addEventListener('close', () => {
		image.removeAttribute('src');
		delete image.dataset.zoomUrl;
		pointers.clear();
		pinchStartDistance = 0;
		resetView();
	});

	// Keyboard-ish interactions ----------------------------------------------
	stage.addEventListener('wheel', (event) => {
		if (!isOpen()) return;
		event.preventDefault();
		setScale(scale * Math.exp(-event.deltaY * 0.0015), event.clientX, event.clientY);
	}, { passive: false });

	dialog.addEventListener('keydown', (event) => {
		if (!isOpen()) return;
		const pan = 60 / Math.max(scale, 1);
		if (event.key === '+' || event.key === '=') {
			event.preventDefault();
			setScale(scale * 1.25);
		} else if (event.key === '-' || event.key === '_') {
			event.preventDefault();
			setScale(scale / 1.25);
		} else if (event.key === '0') {
			event.preventDefault();
			resetView();
		} else if (event.key === 'ArrowLeft') {
			event.preventDefault();
			offsetX += pan;
			applyTransform();
		} else if (event.key === 'ArrowRight') {
			event.preventDefault();
			offsetX -= pan;
			applyTransform();
		} else if (event.key === 'ArrowUp') {
			event.preventDefault();
			offsetY += pan;
			applyTransform();
		} else if (event.key === 'ArrowDown') {
			event.preventDefault();
			offsetY -= pan;
			applyTransform();
		}
	});

	zoomIn.addEventListener('click', () => setScale(scale * 1.25));
	zoomOut.addEventListener('click', () => setScale(scale / 1.25));
	reset.addEventListener('click', resetView);

	// Pointer interactions: one pointer pans, two pointers pinch. Every
	// listener rides on the stage; dispose removes the host wholesale.
	const pointers = new Map();
	let pinchStartDistance = 0;
	let pinchStartScale = 1;

	stage.addEventListener('dblclick', (event) => {
		if (scale === SCALE_MIN) setScale(2.2, event.clientX, event.clientY);
		else resetView();
	});

	stage.addEventListener('pointerdown', (event) => {
		if (!isOpen()) return;
		stage.setPointerCapture?.(event.pointerId);
		pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
		if (pointers.size === 2) {
			const centers = [...pointers.values()];
			pinchStartDistance = Math.hypot(
				centers[0].x - centers[1].x,
				centers[0].y - centers[1].y
			);
			pinchStartScale = scale;
		}
	});

	stage.addEventListener('pointermove', (event) => {
		const previous = pointers.get(event.pointerId);
		if (previous === undefined) return;
		const dx = event.clientX - previous.x;
		const dy = event.clientY - previous.y;
		pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
		if (pointers.size === 2 && pinchStartDistance > 0) {
			const centers = [...pointers.values()];
			const distance = Math.hypot(
				centers[0].x - centers[1].x,
				centers[0].y - centers[1].y
			);
			setScale(pinchStartScale * (distance / pinchStartDistance));
			return;
		}
		if (pointers.size === 1 && scale > SCALE_MIN + 0.01) {
			offsetX += dx;
			offsetY += dy;
			applyTransform();
		}
	});

	const endPointer = (event) => {
		pointers.delete(event.pointerId);
		pinchStartDistance = 0;
	};
	stage.addEventListener('pointerup', endPointer);
	stage.addEventListener('pointercancel', endPointer);

	// Failure keeps a serviceable door open: the direct source link.
	image.addEventListener('error', () => {
		if (!image.getAttribute('src')) return;
		failed.textContent = '';
		const link = document.createElement('a');
		const href = image.dataset.zoomUrl || image.getAttribute('src') || '';
		link.href = href;
		link.textContent = 'Görsel panoda açılamadı; doğrudan bağlantı ile aç.';
		link.target = '_blank';
		link.rel = 'noopener noreferrer';
		failed.append(link);
		failed.classList.remove('hidden');
	});
	image.addEventListener('load', () => {
		if (scale === 1) resetView();
		failed.classList.add('hidden');
	});

	return { open, close: hide, isOpen, dispose };

	function dispose() {
		host.remove();
	}
}
