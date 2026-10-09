/**
 * Browser editing for the three Iced form fields. Rust supplies model values and
 * root-scrollable geometry; persistent DOM nodes own text, selection, composition,
 * and mobile keyboard activation. Render reconciliation hides culled fields and
 * canvas popup overlap without discarding the active editor or caret.
 *
 * This bridge deliberately follows the app's fixed field IDs and Mocha theme.
 */

const fieldIds = ["target-input", "start-input", "reset-input"];
const fields = new Map();
const earlyEvents = [];
let eventSink = null;
let requestedFocus = null;

function emit(kind, id = "", value = "", key = "", modifiers = 0) {
    const event = [kind, id, value, key, modifiers];
    if (eventSink) {
        eventSink(...event);
    } else {
        earlyEvents.push(event);
        if (earlyEvents.length > 256) earlyEvents.shift();
    }
}

/** Attach Rust's event callback, delivering any earlier focus/edit events in order. */
export function setEventSink(sink) {
    eventSink = sink;
    for (const event of earlyEvents.splice(0)) sink(...event);
}

/** Return the active native field ID, or an empty string for canvas/other focus. */
export function activeField() {
    const id = document.activeElement?.id;
    return fields.has(id) ? id : "";
}

/**
 * Apply a reset synchronously, removing pre-reset edit acknowledgements.
 * @param {string[]} values Target, starting value, and reset-index text, in that order.
 */
export function resetPending(values) {
    requestedFocus = null;
    earlyEvents.length = 0;
    for (const [id, field] of fields) {
        const value = values[fieldIds.indexOf(id)];
        field.input.value = value;
        field.pending.length = 0;
        field.modelValue = value;
        field.lastEmitted = value;
        field.composing = false;
    }
}

/** Begin a canvas frame; only fields visited again by syncField may remain visible. */
export function beginFrame() {
    for (const field of fields.values()) {
        field.rendered = false;
        field.occluded = false;
    }
}

/** Hide editors that Iced culled, keeping their nodes and focus for later reveal. */
export function endFrame() {
    for (const field of fields.values()) {
        if (!field.rendered) positionField(field);
    }
}

/**
 * Hide editors intersecting a painted Iced popup until the next normal frame.
 * Coordinates share the logical window space of the supplied field geometry.
 */
export function occludeFields(x, y, width, height) {
    for (const field of fields.values()) {
        if (!field.rendered || !field.geometry) continue;
        const [, , , , clipX, clipY, clipWidth, clipHeight] = field.geometry;
        if (x < clipX + clipWidth && x + width > clipX
            && y < clipY + clipHeight && y + height > clipY) {
            field.occluded = true;
            positionField(field);
        }
    }
}

/**
 * Focus visible editors immediately so trusted taps/Tab retain mobile activation.
 * Other IDs focus the canvas; offscreen field IDs wait until their next visible draw.
 */
export function focus(id) {
    requestedFocus = fieldIds.includes(id) ? id : null;
    const field = fields.get(id);
    const target = field?.input || (!requestedFocus ? document.querySelector("canvas") : null);
    if (target === document.activeElement) {
        requestedFocus = null;
    } else if (target && (!field || field.visible)) {
        target.focus({ preventScroll: true });
    }
}

/** Send whole text before submitting, retaining FIFO values until Rust acknowledges them. */
function emitValue(field, force = false) {
    const value = field.input.value;
    if (force || value !== field.lastEmitted) {
        field.lastEmitted = value;
        field.pending.push(value);
        if (field.pending.length > 128) field.pending.shift();
        emit("changed", field.input.id, value);
    }
}

/** Create an editor once so redraws do not replace its caret, selection, or IME state. */
function createField(id) {
    if (!document.getElementById("native-input-style")) {
        const style = document.createElement("style");
        style.id = "native-input-style";
        style.textContent = fieldIds.map(id => `#${id}::placeholder`).join(",")
            + "{color:#7f849c;opacity:1}";
        document.head.appendChild(style);
    }

    const input = document.createElement("input");
    input.id = id;
    input.type = "text";
    input.inputMode = "numeric";
    input.autocomplete = "off";
    input.autocapitalize = "off";
    input.setAttribute("autocorrect", "off");
    input.spellcheck = false;
    input.enterKeyHint = "go";
    Object.assign(input.style, {
        position: "fixed", zIndex: "30", boxSizing: "border-box", margin: "0",
        padding: "12px 14px", border: "1px solid #45475a", borderRadius: "8px",
        background: "#1e1e2e", color: "#cdd6f4", caretColor: "#cdd6f4",
        font: "20px/24px system-ui, sans-serif", outline: "none", appearance: "none",
    });

    const help = document.createElement("span");
    const error = document.createElement("span");
    help.id = `${id}-native-help`;
    error.id = `${id}-native-error`;
    error.setAttribute("aria-live", "polite");
    for (const description of [help, error]) {
        Object.assign(description.style, {
            position: "fixed", width: "1px", height: "1px", overflow: "hidden",
            clipPath: "inset(50%)", whiteSpace: "nowrap", pointerEvents: "none",
        });
        document.body.appendChild(description);
    }

    document.body.appendChild(input);
    const field = { input, help, error, pending: [], modelValue: "", lastEmitted: "", geometry: null, visible: false, rendered: false, occluded: false, composing: false };
    fields.set(id, field);
    input.addEventListener("input", () => emitValue(field));
    input.addEventListener("compositionstart", () => { field.composing = true; });
    input.addEventListener("compositionend", () => { field.composing = false; emitValue(field); });
    input.addEventListener("focus", () => {
        requestedFocus = null;
        updateBorder(field);
        emit("focused", id);
    });

    input.addEventListener("blur", () => updateBorder(field));
    input.addEventListener("keydown", event => {
        if (event.isComposing || event.keyCode === 229 || field.composing) return;
        const modifiers = Number(event.shiftKey) | Number(event.ctrlKey) << 1
            | Number(event.altKey) << 2 | Number(event.metaKey) << 3;
        if (event.key === "Tab") {
            event.preventDefault();
            emit("key", id, "", "Tab", modifiers);
            const next = fieldIds[fieldIds.indexOf(id) + (event.shiftKey ? -1 : 1)];
            focus(next || "canvas");
        } else if (event.key === "Enter") {
            event.preventDefault();
            emitValue(field, true);
            emit("submit", id);
        } else if (event.key === "Escape") {
            event.preventDefault();
            emit("key", id, "", "Escape", modifiers);
            focus("canvas");
        }
    });

    return field;
}

function updateBorder(field) {
    const focused = document.activeElement === field.input;
    field.input.style.borderColor = field.invalid ? "#f38ba8" : focused ? "#89b4fa" : "#45475a";
    field.input.style.borderWidth = "1px";
}

/**
 * Acknowledge model updates without overwriting newer browser edits. Repeated
 * values consume only the earliest pending prefix. Explicit resets bypass this
 * acknowledgement protocol through resetPending.
 */
function syncValue(field, value) {
    const acknowledged = field.pending.indexOf(value);
    if (acknowledged >= 0) {
        // A repeated value may acknowledge an older edit, not the latest one.
        // Consume only its FIFO prefix and leave the browser's newer text alone.
        field.pending.splice(0, acknowledged + 1);
    } else if (!(field.pending.length && value === field.modelValue) && !field.composing) {
        if (field.input.value !== value) field.input.value = value;
        field.pending.length = 0;
        field.lastEmitted = value;
    }

    field.modelValue = value;
}

/**
 * Map logical canvas geometry to CSS pixels, applying scroll clipping and popup
 * visibility. Recomputing from the canvas rectangle also follows viewport panning.
 */
function positionField(field) {
    const canvas = document.querySelector("canvas");
    if (!canvas || !field.geometry) return;
    const canvasBounds = canvas.getBoundingClientRect();
    const ratio = window.devicePixelRatio || 1;
    const scaleX = canvasBounds.width / (canvas.width / ratio);
    const scaleY = canvasBounds.height / (canvas.height / ratio);
    const [x, y, width, height, clipX, clipY, clipWidth, clipHeight] = field.geometry;
    const left = canvasBounds.left + x * scaleX;
    const top = canvasBounds.top + y * scaleY;
    const rightInset = Math.max(0, x + width - clipX - clipWidth);
    const bottomInset = Math.max(0, y + height - clipY - clipHeight);
    const topInset = Math.max(0, clipY - y);
    const leftInset = Math.max(0, clipX - x);
    field.visible = field.rendered && !field.occluded && clipWidth > 0 && clipHeight > 0;
    Object.assign(field.input.style, {
        left: `${left}px`, top: `${top}px`, width: `${width * scaleX}px`, height: `${height * scaleY}px`,
        fontSize: `${20 * scaleX}px`, lineHeight: `${24 * scaleY}px`,
        padding: `${12 * scaleY}px ${14 * scaleX}px`,
        clipPath: field.visible ? `inset(${topInset * scaleY}px ${rightInset * scaleX}px ${bottomInset * scaleY}px ${leftInset * scaleX}px)` : "inset(50%)",
        opacity: field.visible ? "1" : "0", pointerEvents: field.visible ? "auto" : "none",
    });

    if (requestedFocus === field.input.id && field.visible && document.activeElement !== field.input) {
        field.input.focus({ preventScroll: true });
    }
}

/**
 * Update one persistent editor from a visited Iced field.
 * @param {Array} config ID, label, placeholder, value, invalid flag, help, and error.
 * @param {number[]} geometry x, y, width, height, clipX, clipY, clipWidth, clipHeight.
 */
export function syncField(config, geometry) {
    const [id, label, placeholder, value, invalid, help, error] = config;
    if (!fieldIds.includes(id)) return;
    const field = fields.get(id) || createField(id);
    field.rendered = true;
    field.input.setAttribute("aria-label", label);
    field.input.placeholder = placeholder;
    field.input.setAttribute("aria-invalid", String(invalid));
    field.input.setAttribute("aria-describedby", `${field.help.id}${error ? ` ${field.error.id}` : ""}`);
    if (error) field.input.setAttribute("aria-errormessage", field.error.id);
    else field.input.removeAttribute("aria-errormessage");
    field.help.textContent = help;
    field.error.textContent = error || "";
    field.invalid = invalid;
    syncValue(field, value);
    updateBorder(field);
    field.geometry = geometry;
    positionField(field);
}

function reposition() { for (const field of fields.values()) positionField(field); }
document.addEventListener("pointerdown", () => { requestedFocus = null; }, { capture: true });
window.addEventListener("resize", reposition, { passive: true });
window.visualViewport?.addEventListener("resize", reposition, { passive: true });
window.visualViewport?.addEventListener("scroll", reposition, { passive: true });
