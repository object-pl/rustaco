import { draw_blocks, draw_runs } from './snippets/rustaco-web-5d17a976bd65da1c/inline0.js';


/**
 * Edits for a language module: `numbers` holds 9 values per edit (start, old end, new end
 * in bytes; rows and byte columns of the same three points), `text` the inserted texts
 * concatenated. `stale` means the model's log no longer reaches back to the requested
 * version, and the module must be reset with the full text.
 */
export class EditBatch {
    static __wrap(ptr) {
        const obj = Object.create(EditBatch.prototype);
        obj.__wbg_ptr = ptr;
        EditBatchFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        EditBatchFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_editbatch_free(ptr, 0);
    }
    /**
     * @returns {Uint32Array}
     */
    numbers() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editbatch_numbers(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @returns {boolean}
     */
    stale() {
        const ret = wasm.editbatch_stale(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {string}
     */
    text() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editbatch_text(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The inserted text of each edit.
     * @returns {string[]}
     */
    texts() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editbatch_texts(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayJsValueFromWasm0(r0, r1);
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Four numbers per edit: start line, start UTF-16 character, old end line, old end
     * UTF-16 character; each relative to the text after the edits before it.
     * @returns {Uint32Array}
     */
    utf16() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editbatch_utf16(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}
if (Symbol.dispose) EditBatch.prototype[Symbol.dispose] = EditBatch.prototype.free;

export class Editor {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        EditorFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_editor_free(ptr, 0);
    }
    /**
     * [`Editor::accept_completion`] for an item that replaces `start..end`: the caret's
     * distance to both ends gives what to replace at every cursor.
     * @param {number} start
     * @param {number} end
     * @param {string} text
     * @param {boolean} snippet
     */
    acceptCompletionAt(start, end, text, snippet) {
        const val2 = +start;
        const val3 = +end;
        const val4 = +snippet;
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_acceptCompletionAt(this.__wbg_ptr, val2, val3, ptr0, len0, val4);
    }
    /**
     * Accepts a completion at every cursor: replaces `before` chars ahead of each caret and
     * `after` chars behind it with `text`, as a snippet when `snippet` is true.
     * @param {number} before
     * @param {number} after
     * @param {string} text
     * @param {boolean} snippet
     */
    acceptCompletion(before, after, text, snippet) {
        const val2 = +before;
        const val3 = +after;
        const val4 = +snippet;
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_acceptCompletion(this.__wbg_ptr, val2, val3, ptr0, len0, val4);
    }
    /**
     * The primary caret's line for screen readers, mirrored into the hidden textarea: the
     * line's text (at most 2000 chars around the caret) and the selection in it as UTF-16
     * offsets, start then end (the selection is clipped to the line).
     * @returns {Array<any>}
     */
    accessibleLine() {
        const ret = wasm.editor_accessibleLine(this.__wbg_ptr);
        return ret;
    }
    /**
     * Adds highlights for another run of lines, as [`Editor::set_highlights`] takes them.
     * @param {number} first_line
     * @param {number} end_line
     * @param {Uint32Array | number[]} quads
     */
    addHighlights(first_line, end_line, quads) {
        const val2 = +first_line;
        const val3 = +end_line;
        const ptr0 = passArray32ToWasm0(quads, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_addHighlights(this.__wbg_ptr, val2, val3, ptr0, len0);
    }
    /**
     * Adds keybindings over the current keymap: newline-separated keys (`"ctrl+k ctrl+c"`),
     * commands (`-command` removes a binding) and `when` clauses (empty for none).
     * @param {string} keys
     * @param {string} commands
     * @param {string} whens
     */
    addKeybindings(keys, commands, whens) {
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passStringToWasm0(keys, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(commands, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passStringToWasm0(whens, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_export3(ptr2, len2 * 1, 1); }
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        wasm.editor_addKeybindings(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2);
    }
    /**
     * Adds a view zone (empty space for a DOM node) after line `after_line` (-1: above the
     * first line), `height` pixels tall.
     * @param {number} id
     * @param {number} after_line
     * @param {number} height
     */
    addViewZone(id, after_line, height) {
        wasm.editor_addViewZone(this.__wbg_ptr, id, after_line, height);
    }
    /**
     * Applies edits as one undo step: `ranges` holds start and end char offsets per edit,
     * `texts` the replacement texts. Edits must not overlap.
     * @param {Uint32Array | number[]} ranges
     * @param {string[]} texts
     */
    applyEdits(ranges, texts) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(ranges, wasm.__wbindgen_export);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayJsValueToWasm0(texts, wasm.__wbindgen_export);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export6(ptr1, len1); wasm.__wbindgen_export3(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.editor_applyEdits(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * Whether the primary caret sits inside a string or comment, judged from the highlight
     * styles around it. Used when the language module runs in a worker and cannot answer
     * at once; auto-closing pairs stay closed inside literals.
     * @returns {boolean}
     */
    caretInLiteral() {
        const ret = wasm.editor_caretInLiteral(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Primary caret position in CSS pixels, for placing the hidden textarea (IME popups).
     * @returns {number}
     */
    caret_x() {
        const ret = wasm.editor_caret_x(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    caret_y() {
        const ret = wasm.editor_caret_y(this.__wbg_ptr);
        return ret;
    }
    /**
     * Width of one cell in CSS pixels.
     * @returns {number}
     */
    charWidth() {
        const ret = wasm.editor_charWidth(this.__wbg_ptr);
        return ret;
    }
    clearFind() {
        wasm.editor_clearFind(this.__wbg_ptr);
    }
    /**
     * Drops highlights, e.g. when the model's language is turned off.
     */
    clearHighlights() {
        wasm.editor_clearHighlights(this.__wbg_ptr);
    }
    clearTreeBrackets() {
        wasm.editor_clearTreeBrackets(this.__wbg_ptr);
    }
    /**
     * What a copy or cut puts on the clipboard next to the text so a paste can go back one
     * piece per cursor: JSON `{ pieces }` when there are several selections, else `""`.
     * @returns {string}
     */
    clipboardMeta() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_clipboardMeta(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The filter texts of the completion items at `indices`, NUL-separated.
     * @param {Uint32Array | number[]} indices
     * @returns {string}
     */
    completionTexts(indices) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const ptr0 = passArray32ToWasm0(indices, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.editor_completionTexts(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * One step of an IME composition: replaces the `before` chars ahead of each caret (the
     * previous step's text, 0 at the start) with `text` and underlines it.
     * @param {number} before
     * @param {string} text
     */
    compose(before, text) {
        const val2 = +before;
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_compose(this.__wbg_ptr, val2, ptr0, len0);
    }
    /**
     * The selections as HTML with syntax colours, for copying into rich text editors.
     * Colours come from the highlights of the lines on screen; text beyond them is plain.
     * @returns {string}
     */
    copyHtml() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_copyHtml(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {string}
     */
    copy() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_copy(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {string}
     */
    cut() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_cut(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * One step of scrolling while a drag selection is held past the top or bottom edge,
     * faster the further out the pointer is (at most 100 pixels a step); the selection
     * follows the pointer. False when the pointer is back inside, so the timer can stop.
     * @param {number} x
     * @param {number} y
     * @returns {boolean}
     */
    dragAutoscroll(x, y) {
        const ret = wasm.editor_dragAutoscroll(this.__wbg_ptr, x, y);
        return ret !== 0;
    }
    /**
     * Where dragged text would drop, while a text drag is under way.
     * @returns {number | undefined}
     */
    dropPosition() {
        const ret = wasm.editor_dropPosition(this.__wbg_ptr);
        return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
    }
    /**
     * Ends the composition: its text stays, the underline goes.
     */
    endComposition() {
        wasm.editor_endComposition(this.__wbg_ptr);
    }
    /**
     * Expands the primary selection to the next enclosing syntax node. `flat` holds node
     * ranges from the language module as (start, end) pairs, innermost first.
     * @param {Uint32Array | number[]} flat
     * @returns {boolean}
     */
    expandSelection(flat) {
        const ptr0 = passArray32ToWasm0(flat, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_expandSelection(this.__wbg_ptr, ptr0, len0);
        return ret !== 0;
    }
    /**
     * Selects the next (or previous) match after the primary selection, wrapping around.
     * @param {boolean} forward
     * @returns {boolean}
     */
    findNext(forward) {
        const ret = wasm.editor_findNext(this.__wbg_ptr, forward);
        return ret !== 0;
    }
    /**
     * Which match the primary selection is (1-based, 0 when it is none) and how many
     * there are, for "3 of 12".
     * @returns {Uint32Array}
     */
    findPosition() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_findPosition(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Searches the model and highlights every match. Returns the number of matches, or -1
     * when the regular expression does not compile.
     * @param {string} pattern
     * @param {boolean} regex
     * @param {boolean} case_sensitive
     * @param {boolean} whole_word
     * @returns {number}
     */
    find(pattern, regex, case_sensitive, whole_word) {
        const val2 = +regex;
        const val3 = +case_sensitive;
        const val4 = +whole_word;
        const ptr0 = passStringToWasm0(pattern, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_find(this.__wbg_ptr, ptr0, len0, val2, val3, val4);
        return ret;
    }
    /**
     * Makes the next [`Editor::highlight_request`] ask again (a background parse finished).
     */
    forgetHighlightRequest() {
        wasm.editor_forgetHighlightRequest(this.__wbg_ptr);
    }
    /**
     * Makes the next frame clear the atlas and draw its glyphs again (after the WebGL
     * context was restored).
     */
    glForgetAtlas() {
        wasm.editor_glForgetAtlas(this.__wbg_ptr);
    }
    /**
     * Slot width, height, left padding and slots per row of the atlas, the atlas side, and 1
     * when the atlas was cleared this frame.
     * @returns {Uint32Array}
     */
    glGeometry() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_glGeometry(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Quads of the last frame: pointer into WASM memory and count (eight 32-bit words each:
     * x, y, width, height, slot origin x and y as f32 device pixels, atlas slot, RGBA).
     * @returns {Uint32Array}
     */
    glInstances() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_glInstances(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The chars of the pending glyphs, one per glyph.
     * @returns {string}
     */
    glPendingChars() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_glPendingChars(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Glyphs to draw into the atlas (slot, font variant and quarter-pixel offset per glyph),
     * taken: the next call returns only glyphs laid out since. Read [`Editor::gl_geometry`]
     * and [`Editor::gl_pending_chars`] first, as this clears the reset flag and the chars.
     * @returns {Uint32Array}
     */
    glTakePending() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_glTakePending(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * True while the first key of a chord (`ctrl+k`) waits for the second.
     * @returns {boolean}
     */
    hasPendingChord() {
        const ret = wasm.editor_hasPendingChord(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * What a frame needs from the language module, once per change of text, visible lines,
     * caret or bracket colouring: the caret offset followed by the visible runs of lines
     * (pairs of first and end line). Empty when nothing changed since the last call, or
     * since [`Editor::forget_highlight_request`].
     * @param {boolean} colorize
     * @returns {Uint32Array}
     */
    highlightRequest(colorize) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_highlightRequest(retptr, this.__wbg_ptr, colorize);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * X of the first non-blank char of `line`, from the text area's left edge, for placing
     * widgets above a line's code.
     * @param {number} line
     * @returns {number}
     */
    indentOfLine(line) {
        const ret = wasm.editor_indentOfLine(this.__wbg_ptr, line);
        return ret;
    }
    /**
     * Inserts a snippet (VS Code syntax) at every selection and selects its first tab stop.
     * @param {string} source
     */
    insertSnippet(source) {
        const ptr0 = passStringToWasm0(source, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_insertSnippet(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Typed text from the input element. A single char goes through bracket handling
     * (auto-closing pairs, typing over a closing bracket), except that `in_literal` (the
     * cursor is inside a string or comment) opens no pairs; longer text (IME, dictation) is
     * inserted as is.
     * @param {string} text
     * @param {boolean} in_literal
     */
    insert_text(text, in_literal) {
        const val2 = +in_literal;
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_insert_text(this.__wbg_ptr, ptr0, len0, val2);
    }
    /**
     * Whether the point is inside a non-empty selection (a right click there keeps it).
     * @param {number} x
     * @param {number} y
     * @returns {boolean}
     */
    insideSelection(x, y) {
        const ret = wasm.editor_insideSelection(this.__wbg_ptr, x, y);
        return ret !== 0;
    }
    /**
     * True for commands the frontend runs itself (they need the language module, providers
     * or the DOM); `key_down` reports these instead of running them.
     * @param {string} command
     * @returns {boolean}
     */
    isFrontendCommand(command) {
        const ptr0 = passStringToWasm0(command, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_isFrontendCommand(this.__wbg_ptr, ptr0, len0);
        return ret !== 0;
    }
    /**
     * [`Editor::key_down`] for a DOM key event. With Shift a symbol key types another char
     * (`|` for `\` on US layouts), so it is named by its physical `code` instead, as VS Code
     * does; otherwise the produced char counts, which keeps bindings working on other layouts.
     * @param {string} key
     * @param {string} code
     * @param {boolean} ctrl
     * @param {boolean} shift
     * @param {boolean} alt
     * @param {boolean} meta
     * @returns {number}
     */
    keyDownEvent(key, code, ctrl, shift, alt, meta) {
        const val3 = +ctrl;
        const val4 = +shift;
        const val5 = +alt;
        const val6 = +meta;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(key, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(code, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.editor_keyDownEvent(this.__wbg_ptr, ptr0, len0, ptr1, len1, val3, val4, val5, val6);
        return ret;
    }
    /**
     * Handles a keydown through the keymap: 0 when the key is not bound (printable text,
     * AltGr characters arrive through [`Editor::insert_text`]), 1 when it was handled, 2 when
     * it resolved to a command the JS side must run ([`Editor::take_frontend_command`]).
     * @param {string} key
     * @param {boolean} ctrl
     * @param {boolean} shift
     * @param {boolean} alt
     * @param {boolean} meta
     * @returns {number}
     */
    key_down(key, ctrl, shift, alt, meta) {
        const val2 = +ctrl;
        const val3 = +shift;
        const val4 = +alt;
        const val5 = +meta;
        const ptr0 = passStringToWasm0(key, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_key_down(this.__wbg_ptr, ptr0, len0, val2, val3, val4, val5);
        return ret;
    }
    /**
     * @returns {number}
     */
    line_height() {
        const ret = wasm.editor_line_height(this.__wbg_ptr);
        return ret;
    }
    /**
     * Ends a press. A text drag drops its text where the pointer is: moved, or copied
     * with `copy` (Ctrl, or Alt on macOS, as in VS Code). A press inside a selection that
     * did not move places the caret there.
     * @param {boolean} copy
     */
    mouseUpWith(copy) {
        wasm.editor_mouseUpWith(this.__wbg_ptr, copy);
    }
    /**
     * `clicks` is the DOM `detail`: 1 places a cursor, 2 selects a word, 3 a line.
     * @param {number} x
     * @param {number} y
     * @param {number} clicks
     * @param {boolean} shift
     * @param {boolean} alt
     */
    mouse_down(x, y, clicks, shift, alt) {
        wasm.editor_mouse_down(this.__wbg_ptr, x, y, clicks, shift, alt);
    }
    /**
     * Extends the selection while a button is held. Returns false when no drag is active.
     * @param {number} x
     * @param {number} y
     * @returns {boolean}
     */
    mouse_move(x, y) {
        const ret = wasm.editor_mouse_move(this.__wbg_ptr, x, y);
        return ret !== 0;
    }
    mouse_up() {
        wasm.editor_mouse_up(this.__wbg_ptr);
    }
    /**
     * @param {HTMLCanvasElement} canvas
     * @param {Model} model
     * @param {string} font_family
     * @param {number} font_size
     */
    constructor(canvas, model, font_family, font_size) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(model, Model);
            const val2 = +font_size;
            const ptr0 = passStringToWasm0(font_family, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.editor_new(retptr, canvas, model.__wbg_ptr, ptr0, len0, val2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeFromExternrefTable0(r1);
            }
            this.__wbg_ptr = r0;
            EditorFinalization.register(this, this.__wbg_ptr, this);
            return this;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Pastes clipboard `text`. With `meta` from [`Editor::clipboard_meta`] whose pieces match
     * the text and the cursor count, each cursor gets its piece; otherwise every cursor gets
     * the whole text.
     * @param {string} text
     * @param {string} meta
     */
    pasteClipboard(text, meta) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(meta, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        wasm.editor_pasteClipboard(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * Pastes one piece per cursor (as copied from that many selections), or the pieces
     * joined when the counts differ.
     * @param {string[]} pieces
     */
    pasteEach(pieces) {
        const ptr0 = passArrayJsValueToWasm0(pieces, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_pasteEach(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @param {string} text
     */
    paste(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_paste(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Where char `offset` is on screen: x, y and line height, or empty when its line is
     * not visible. Content widgets are placed with it.
     * @param {number} offset
     * @returns {Float64Array}
     */
    positionOf(offset) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_positionOf(retptr, this.__wbg_ptr, offset);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayF64FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 8, 8);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The primary selection as [start, end] char offsets.
     * @returns {Uint32Array}
     */
    primaryRange() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_primaryRange(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Filters and ranks the completion items by what was typed, best first (in the given
     * order with `keep_order`, for snippet choices). The number of matches, then for at most
     * `limit` of them: item index, number of matched chars, then their char indices.
     * @param {string} typed
     * @param {boolean} keep_order
     * @param {number} limit
     * @returns {Uint32Array}
     */
    rankCompletions(typed, keep_order, limit) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const val2 = +keep_order;
            const val3 = +limit;
            const ptr0 = passStringToWasm0(typed, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.editor_rankCompletions(retptr, this.__wbg_ptr, ptr0, len0, val2, val3);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v4 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v4;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The completion list after a keystroke: the text from `start` (the word start) to the
     * caret filters and ranks the items. Answers `[0]` when the list should close (the caret
     * left the word, a non-word char was typed, nothing matches, or without `explicit` the
     * only match is already typed in full). Otherwise 1, the number of matches, then for at
     * most `limit` of them: item index, number of matched chars and their indices in the
     * label (none when the item filters by a text other than its label). With `keep_order`
     * (a snippet's choices) nothing is typed yet and the items keep their order.
     * @param {number} start
     * @param {boolean} keep_order
     * @param {boolean} explicit
     * @param {number} limit
     * @returns {Uint32Array}
     */
    refilterCompletions(start, keep_order, explicit, limit) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_refilterCompletions(retptr, this.__wbg_ptr, start, keep_order, explicit, limit);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Re-runs the current query, after the text changed. Returns the number of matches.
     * @returns {number}
     */
    refreshFind() {
        const ret = wasm.editor_refreshFind(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Recomputes the regions that can be folded, from indentation. The JS side calls it
     * once typing pauses; with a syntax tree it passes ranges through
     * [`Editor::set_folding_ranges`] instead.
     */
    refreshFoldingRanges() {
        wasm.editor_refreshFoldingRanges(this.__wbg_ptr);
    }
    /**
     * Recomputes the overview ruler marks at the next frame. The JS side calls it once
     * typing pauses and after decorations or matches change.
     */
    refreshOverview() {
        wasm.editor_refreshOverview(this.__wbg_ptr);
    }
    /**
     * Makes keybindings that resolve to `id` hand it to the JS side, where the embedder's
     * handler runs (see `addCommand` in rustaco.js).
     * @param {string} id
     */
    registerHostCommand(id) {
        const ptr0 = passStringToWasm0(id, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_registerHostCommand(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @param {number} id
     * @returns {boolean}
     */
    removeViewZone(id) {
        const ret = wasm.editor_removeViewZone(this.__wbg_ptr, id);
        return ret !== 0;
    }
    /**
     * @param {boolean} caret_visible
     */
    render(caret_visible) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_render(retptr, this.__wbg_ptr, caret_visible);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeFromExternrefTable0(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Replaces every match as one undo step. Returns the number of replacements.
     * @param {string} replacement
     * @returns {number}
     */
    replaceAll(replacement) {
        const ptr0 = passStringToWasm0(replacement, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_replaceAll(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * Replaces the selected match and moves to the next one.
     * @param {string} replacement
     * @returns {boolean}
     */
    replaceOne(replacement) {
        const ptr0 = passStringToWasm0(replacement, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_replaceOne(this.__wbg_ptr, ptr0, len0);
        return ret !== 0;
    }
    /**
     * @param {number} width
     * @param {number} height
     * @param {number} dpr
     */
    resize(width, height, dpr) {
        wasm.editor_resize(this.__wbg_ptr, width, height, dpr);
    }
    /**
     * Runs a command by id (see the keymap). Returns false for unknown ids.
     * @param {string} command
     * @returns {boolean}
     */
    runCommand(command) {
        const ptr0 = passStringToWasm0(command, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_runCommand(this.__wbg_ptr, ptr0, len0);
        return ret !== 0;
    }
    /**
     * Vertical scroll position in CSS pixels.
     * @returns {number}
     */
    scrollTop() {
        const ret = wasm.editor_scrollTop(this.__wbg_ptr);
        return ret;
    }
    /**
     * Scrolls to the top; used after the model's text is replaced.
     */
    scroll_to_top() {
        wasm.editor_scroll_to_top(this.__wbg_ptr);
    }
    /**
     * Selects every match (one cursor per match).
     * @returns {number}
     */
    selectAllMatches() {
        const ret = wasm.editor_selectAllMatches(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The single-line text of the primary selection, to seed the find widget; empty
     * when the selection is empty or spans lines.
     * @returns {string}
     */
    selectionSeed() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_selectionSeed(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The text of each non-empty selection, for clipboard metadata.
     * @returns {string[]}
     */
    selectionTexts() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_selectionTexts(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayJsValueFromWasm0(r0, r1);
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Selections as a flat list of char offsets: anchor, head, anchor, head, ...
     * The primary selection comes first.
     * @returns {Uint32Array}
     */
    selections() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_selections(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Sets the items of the completion list, for [`Editor::rank_completions`]: per item the
     * text it is filtered by (filter text or label) and its sort text, NUL-separated.
     * @param {string} texts
     * @param {string} sort_texts
     * @param {string} labels
     */
    setCompletionItems(texts, sort_texts, labels) {
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passStringToWasm0(texts, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(sort_texts, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passStringToWasm0(labels, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_export3(ptr2, len2 * 1, 1); }
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        wasm.editor_setCompletionItems(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2);
    }
    /**
     * Shows matches found elsewhere (pairs of start and end char offsets) for the current
     * query; they move with edits like any decoration.
     * @param {Uint32Array | number[]} pairs
     * @returns {number}
     */
    setFindMatches(pairs) {
        const ptr0 = passArray32ToWasm0(pairs, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_setFindMatches(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * Sets the query without searching, for when the matches come from the worker
     * ([`Editor::set_find_matches`]). Returns 0, or -1 when the regular expression does
     * not compile.
     * @param {string} pattern
     * @param {boolean} regex
     * @param {boolean} case_sensitive
     * @param {boolean} whole_word
     * @returns {number}
     */
    setFindQuery(pattern, regex, case_sensitive, whole_word) {
        const val2 = +regex;
        const val3 = +case_sensitive;
        const val4 = +whole_word;
        const ptr0 = passStringToWasm0(pattern, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_setFindQuery(this.__wbg_ptr, ptr0, len0, val2, val3, val4);
        return ret;
    }
    /**
     * Sets the regions that can be folded: pairs of header line and last line, from a syntax
     * tree. With `with_indentation`, regions from indentation are added where no pair starts
     * on the same line (Python, YAML: blocks without brackets).
     * @param {Uint32Array | number[]} pairs
     * @param {boolean} with_indentation
     */
    setFoldingRanges(pairs, with_indentation) {
        const val2 = +with_indentation;
        const ptr0 = passArray32ToWasm0(pairs, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_setFoldingRanges(this.__wbg_ptr, ptr0, len0, val2);
    }
    /**
     * Changes the font. The cell width is measured again, the line height follows the size,
     * wrapping and line heights are recomputed at the next frame, the glyph atlas starts
     * over, and the line at the top of the viewport stays there. Text sits on a grid of equal
     * cells, so only monospace fonts line up.
     * @param {string} font_family
     * @param {number} font_size
     */
    setFont(font_family, font_size) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const val2 = +font_size;
            const ptr0 = passStringToWasm0(font_family, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.editor_setFont(retptr, this.__wbg_ptr, ptr0, len0, val2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeFromExternrefTable0(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Takes highlights from a language module: quads of line, start column, end column
     * (chars) and style id, covering lines from `first_line`. Lines without quads are plain.
     * Replaces all highlights; [`Editor::add_highlights`] adds another run of lines.
     * @param {number} first_line
     * @param {number} end_line
     * @param {Uint32Array | number[]} quads
     */
    setHighlights(first_line, end_line, quads) {
        const val2 = +first_line;
        const val3 = +end_line;
        const ptr0 = passArray32ToWasm0(quads, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_setHighlights(this.__wbg_ptr, val2, val3, ptr0, len0);
    }
    /**
     * Shows inlay hints: `labels[i]` before the char at `offsets[i]`.
     * @param {Uint32Array | number[]} offsets
     * @param {string[]} labels
     */
    setInlayHints(offsets, labels) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(offsets, wasm.__wbindgen_export);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayJsValueToWasm0(labels, wasm.__wbindgen_export);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export6(ptr1, len1); wasm.__wbindgen_export3(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.editor_setInlayHints(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * Selects the keymap: `preset` is `"default"` (VS Code) or `"jetbrains"`, `platform` is
     * `"mac"`, `"windows"` or `"linux"`.
     * @param {string} preset
     * @param {string} platform
     * @returns {boolean}
     */
    setKeymap(preset, platform) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(preset, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(platform, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.editor_setKeymap(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        return ret !== 0;
    }
    /**
     * Shows or hides the minimap.
     * @param {boolean} on
     */
    setMinimap(on) {
        wasm.editor_setMinimap(this.__wbg_ptr, on);
    }
    /**
     * @param {number} top
     */
    setScrollTop(top) {
        wasm.editor_setScrollTop(this.__wbg_ptr, top);
    }
    /**
     * [`Editor::set_semantic_tokens`] with style names: `ranges` holds start and end per
     * token, `styles` the highlight names joined by newlines (`"function\nvariable"`).
     * @param {Uint32Array | number[]} ranges
     * @param {string} styles
     */
    setSemanticTokensNamed(ranges, styles) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(ranges, wasm.__wbindgen_export);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(styles, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.editor_setSemanticTokensNamed(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * Takes semantic tokens: triples of start and end char offsets and style id
     * ([`style_id`]), drawn over the syntax highlights until replaced.
     * @param {Uint32Array | number[]} triples
     */
    setSemanticTokens(triples) {
        const ptr0 = passArray32ToWasm0(triples, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_setSemanticTokens(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Pins up to `max` headers of the enclosing scopes at the top; 0 turns it off.
     * @param {number} max
     */
    setStickyScroll(max) {
        wasm.editor_setStickyScroll(this.__wbg_ptr, max);
    }
    /**
     * Paints frames with WebGL (`"webgl"`) or on the canvas (`"canvas"`, the default). With
     * WebGL, JS draws each frame from [`Editor::gl_instances`] after [`Editor::render`].
     * @param {string} kind
     */
    setTextLayer(kind) {
        const ptr0 = passStringToWasm0(kind, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_setTextLayer(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * The bracket pair at the cursor from the language module's tree ([open, close], or empty
     * for none). Without this call the editor matches brackets by scanning the text.
     * @param {Uint32Array | number[]} pair
     */
    setTreeBrackets(pair) {
        const ptr0 = passArray32ToWasm0(pair, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_setTreeBrackets(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * [`Editor::set_folding_ranges`] for regions from the syntax tree: languages whose blocks
     * open after a line ending in `:` (Python, YAML; `indent_after` in the catalog) also get
     * the regions from indentation.
     * @param {Uint32Array | number[]} pairs
     */
    setTreeFoldingRanges(pairs) {
        const ptr0 = passArray32ToWasm0(pairs, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_setTreeFoldingRanges(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Display options: ruler columns, whitespace marks, indentation guides, highlighting of
     * other occurrences of the word at the cursor.
     * @param {Uint32Array | number[]} rulers
     * @param {boolean} render_whitespace
     * @param {boolean} guides
     * @param {boolean} occurrences
     */
    setViewOptions(rulers, render_whitespace, guides, occurrences) {
        const val2 = +render_whitespace;
        const val3 = +guides;
        const val4 = +occurrences;
        const ptr0 = passArray32ToWasm0(rulers, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_setViewOptions(this.__wbg_ptr, ptr0, len0, val2, val3, val4);
    }
    /**
     * Makes the words around `line` the completion items (for languages without a
     * completion provider), without `exclude`; they stay in Rust, and the list asks for the
     * texts of the few it shows with [`Editor::completion_texts`]. Returns their number.
     * @param {Model} model
     * @param {number} line
     * @param {number} radius
     * @param {number} max
     * @param {string} exclude
     * @returns {number}
     */
    setWordCompletions(model, line, radius, max, exclude) {
        _assertClass(model, Model);
        const val2 = +line;
        const val3 = +radius;
        const val4 = +max;
        const ptr0 = passStringToWasm0(exclude, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.editor_setWordCompletions(this.__wbg_ptr, model.__wbg_ptr, val2, val3, val4, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * Turns word wrap at the editor's width on or off.
     * @param {boolean} on
     */
    setWordWrap(on) {
        wasm.editor_setWordWrap(this.__wbg_ptr, on);
    }
    /**
     * Replaces the selections; takes the same flat layout [`Editor::selections`] returns.
     * @param {Uint32Array | number[]} flat
     */
    set_selections(flat) {
        const ptr0 = passArray32ToWasm0(flat, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.editor_set_selections(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * The options of the snippet stop being edited, when it is a choice.
     * @returns {string[]}
     */
    snippetChoices() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_snippetChoices(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayJsValueFromWasm0(r0, r1);
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The command [`Editor::key_down`] left for the JS side, once.
     * @returns {string}
     */
    takeFrontendCommand() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_takeFrontendCommand(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Whether highlights or the bracket pair set since the last call differ from what was
     * drawn; clears the flag. The JS side draws a frame for a worker reply only then.
     * @returns {boolean}
     */
    takeHighlightsChanged() {
        const ret = wasm.editor_takeHighlightsChanged(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * The char offset under a point in canvas coordinates, or -1 when the point is not over
     * text (the gutter, past a line's end, the minimap).
     * @param {number} x
     * @param {number} y
     * @returns {number}
     */
    textOffsetAt(x, y) {
        const ret = wasm.editor_textOffsetAt(this.__wbg_ptr, x, y);
        return ret;
    }
    /**
     * Folds or unfolds the region headed at `line`, as a click on the gutter marker does.
     * @param {number} line
     * @returns {boolean}
     */
    toggleFold(line) {
        const ret = wasm.editor_toggleFold(this.__wbg_ptr, line);
        return ret !== 0;
    }
    /**
     * View zones drawn in the last frame: id, x, y, width and height for each.
     * @returns {Float64Array}
     */
    viewZoneRects() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_viewZoneRects(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayF64FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 8, 8);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The lines on screen as pairs of first and end line (exclusive), one pair per run of
     * lines not hidden by a fold.
     * @returns {Uint32Array}
     */
    visibleLines() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_visibleLines(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * A wheel event: `mode` is the DOM's `deltaMode` (0 pixels, 1 lines, 2 pages). With
     * Shift and no horizontal delta, the vertical delta scrolls sideways.
     * @param {number} dx
     * @param {number} dy
     * @param {number} mode
     * @param {boolean} shift
     */
    wheelEvent(dx, dy, mode, shift) {
        wasm.editor_wheelEvent(this.__wbg_ptr, dx, dy, mode, shift);
    }
    /**
     * @param {number} dx
     * @param {number} dy
     */
    wheel(dx, dy) {
        wasm.editor_wheel(this.__wbg_ptr, dx, dy);
    }
    /**
     * The word at the primary caret: [start, end] char offsets (equal when there is none).
     * @returns {Uint32Array}
     */
    wordAtCaret() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.editor_wordAtCaret(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}
if (Symbol.dispose) Editor.prototype[Symbol.dispose] = Editor.prototype.free;

/**
 * One language server connection's protocol state.
 */
export class LspSession {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        LspSessionFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_lspsession_free(ptr, 0);
    }
    /**
     * Drops a request and tells the server; its answer, if it comes, is ignored.
     * @param {number} id
     */
    cancel(id) {
        wasm.lspsession_cancel(this.__wbg_ptr, id);
    }
    /**
     * The capabilities the server announced, as JSON.
     * @returns {string}
     */
    capabilities() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.lspsession_capabilities(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} doc
     */
    close(doc) {
        wasm.lspsession_close(this.__wbg_ptr, doc);
    }
    /**
     * The latest diagnostics for `uri` against `model`, as JSON:
     * `[{ start, end, severity, message }]`.
     * @param {string} uri
     * @param {Model} model
     * @returns {string}
     */
    diagnostics(uri, model) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(model, Model);
            const ptr0 = passStringToWasm0(uri, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.lspsession_diagnostics(retptr, this.__wbg_ptr, ptr0, len0, model.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Queues `initialize`; returns the request id. Its result is the server's capabilities.
     * @param {string | null} [root_uri]
     * @returns {number}
     */
    initialize(root_uri) {
        var ptr0 = isLikeNone(root_uri) ? 0 : passStringToWasm0(root_uri, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.lspsession_initialize(this.__wbg_ptr, ptr0, len0);
        return ret;
    }
    constructor() {
        const ret = wasm.lspsession_new();
        this.__wbg_ptr = ret;
        LspSessionFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Queues a notification with JSON params.
     * @param {string} method
     * @param {string} params
     */
    notify(method, params) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(method, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(params, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        wasm.lspsession_notify(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * Opens `model` as `uri`; returns a document handle.
     * @param {Model} model
     * @param {string} uri
     * @param {string} language_id
     * @returns {number}
     */
    open(model, uri, language_id) {
        _assertClass(model, Model);
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(uri, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(language_id, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.lspsession_open(this.__wbg_ptr, model.__wbg_ptr, ptr0, len0, ptr1, len1);
        return ret >>> 0;
    }
    /**
     * Takes in one message from the server and says what it was, as JSON:
     * `{ type: "response", id, error? }` (the result waits for [`LspSession::result`]),
     * `{ type: "diagnostics", uri }`, `{ type: "applyEdit", id, edit }`,
     * `{ type: "notification", method, params }` for any other notification, or
     * `{ type: "other" }`. Other server requests are answered with null here.
     * @param {string} text
     * @returns {string}
     */
    receive(text) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.lspsession_receive(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Queues any request with JSON params; its result comes back as the server sent it.
     * @param {string} method
     * @param {string} params
     * @returns {number}
     */
    requestRaw(method, params) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(method, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(params, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.lspsession_requestRaw(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        return ret;
    }
    /**
     * Syncs, then queues a request about `doc`; returns its id. `kind` names the editor
     * feature; `a` and `b` are offsets (a position, or a range's ends) and `text` the extra
     * argument (trigger character, new name). Formatting takes the tab size in `a` and
     * spaces (1) or tabs (0) in `b`.
     * @param {string} kind
     * @param {number} doc
     * @param {Model} model
     * @param {number} a
     * @param {number} b
     * @param {string | null} [text]
     * @returns {number}
     */
    request(kind, doc, model, a, b, text) {
        _assertClass(model, Model);
        const val3 = +doc;
        const val4 = +a;
        const val5 = +b;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(kind, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = isLikeNone(text) ? 0 : passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.lspsession_request(this.__wbg_ptr, ptr0, len0, val3, model.__wbg_ptr, val4, val5, ptr1, len1);
        return ret;
    }
    /**
     * Answers a request the server sent, with a JSON result.
     * @param {string} id
     * @param {string} result
     */
    respond(id, result) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(id, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(result, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
            throw e;
        }
        wasm.lspsession_respond(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * The result of `initialize` or a raw request, as JSON; the request is forgotten.
     * @param {number} id
     * @returns {string}
     */
    resultRaw(id) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.lspsession_resultRaw(retptr, this.__wbg_ptr, id);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The decoded result of a request about a document whose answer arrived, as JSON,
     * against `model` (the document asked about); the request is forgotten. `null` when the
     * model changed since the request, since the answer's positions are about the old text.
     * @param {number} id
     * @param {Model} model
     * @returns {string}
     */
    result(id, model) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(model, Model);
            wasm.lspsession_result(retptr, this.__wbg_ptr, id, model.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Queues a `didChange` when the model moved on since the server last saw it.
     * @param {number} doc
     * @param {Model} model
     */
    sync(doc, model) {
        _assertClass(model, Model);
        wasm.lspsession_sync(this.__wbg_ptr, doc, model.__wbg_ptr);
    }
    /**
     * Messages to send, oldest first; the queue empties.
     * @returns {string[]}
     */
    take() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.lspsession_take(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayJsValueFromWasm0(r0, r1);
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The char offset of an LSP position (JSON) in `model`.
     * @param {Model} model
     * @param {string} position
     * @returns {number}
     */
    toOffset(model, position) {
        _assertClass(model, Model);
        const ptr0 = passStringToWasm0(position, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.lspsession_toOffset(this.__wbg_ptr, model.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * The LSP position (JSON) of a char offset in `model`, in the negotiated encoding.
     * @param {Model} model
     * @param {number} offset
     * @returns {string}
     */
    toPosition(model, offset) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(model, Model);
            wasm.lspsession_toPosition(retptr, this.__wbg_ptr, model.__wbg_ptr, offset);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The char range `[start, end]` of an LSP range (JSON) in `model`.
     * @param {string} range
     * @param {Model} model
     * @returns {Uint32Array}
     */
    toRange(range, model) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(model, Model);
            const ptr0 = passStringToWasm0(range, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.lspsession_toRange(retptr, this.__wbg_ptr, ptr0, len0, model.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v2 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v2;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Char ranges of many LSP ranges (a JSON array) in `model`: start and end per range.
     * @param {string} ranges
     * @param {Model} model
     * @returns {Uint32Array}
     */
    toRanges(ranges, model) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(model, Model);
            const ptr0 = passStringToWasm0(ranges, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.lspsession_toRanges(retptr, this.__wbg_ptr, ptr0, len0, model.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v2 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v2;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The edits of a `WorkspaceEdit` (JSON) grouped by document, from `changes` and
     * `documentChanges` alike, as JSON `[{ uri, edits }]` with the edits as LSP sent them.
     * @param {string} edit
     * @returns {string}
     */
    workspaceEditGroups(edit) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const ptr0 = passStringToWasm0(edit, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.lspsession_workspaceEditGroups(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1);
        }
    }
}
if (Symbol.dispose) LspSession.prototype[Symbol.dispose] = LspSession.prototype.free;

export class Model {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ModelFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_model_free(ptr, 0);
    }
    /**
     * Length of the text in chars (JS's `length` counts UTF-16 units).
     * @returns {number}
     */
    charCount() {
        const ret = wasm.model_charCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Language id for a file path and first line, or `""` when none matches.
     * @param {string} path
     * @param {string} first_line
     * @returns {string}
     */
    static detectLanguage(path, first_line) {
        let deferred3_0;
        let deferred3_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            let ptr0, len0, ptr1, len1;
            try {
                ptr0 = passStringToWasm0(path, wasm.__wbindgen_export, wasm.__wbindgen_export2);
                len0 = WASM_VECTOR_LEN;
                ptr1 = passStringToWasm0(first_line, wasm.__wbindgen_export, wasm.__wbindgen_export2);
                len1 = WASM_VECTOR_LEN;
            } catch (e) {
                if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
                if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
                throw e;
            }
            wasm.model_detectLanguage(retptr, ptr0, len0, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred3_0 = r0;
            deferred3_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * The ranges a diff view marks, this model being the original: triples of kind (0
     * removed line, 1 inserted line, 2 removed chars, 3 inserted chars), start and end. The
     * removed kinds are offsets into this model, the inserted ones into `modified`.
     * @param {Uint32Array | number[]} flat
     * @param {Model} modified
     * @param {boolean} inline
     * @returns {Uint32Array}
     */
    diffMarks(flat, modified, inline) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(modified, Model);
            const val2 = +inline;
            const ptr0 = passArray32ToWasm0(flat, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.model_diffMarks(retptr, this.__wbg_ptr, ptr0, len0, modified.__wbg_ptr, val2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v3 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v3;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The edit to `modified` that reverts change `index`, this model being the original:
     * JSON `{ start, end, text }`, or `null` for a missing change.
     * @param {Uint32Array | number[]} flat
     * @param {number} index
     * @param {Model} modified
     * @returns {string}
     */
    diffRevertEdit(flat, index, modified) {
        let deferred3_0;
        let deferred3_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(modified, Model);
            const val2 = +index;
            const ptr0 = passArray32ToWasm0(flat, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.model_diffRevertEdit(retptr, this.__wbg_ptr, ptr0, len0, val2, modified.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred3_0 = r0;
            deferred3_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * Diffs this model (the original) against `modified` on the calling thread; the result
     * is laid out as `TextReplica.diff` returns it.
     * @param {Model} modified
     * @param {boolean} ignore_whitespace
     * @param {number} budget_ms
     * @returns {Uint32Array}
     */
    diffWith(modified, ignore_whitespace, budget_ms) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(modified, Model);
            wasm.model_diffWith(retptr, this.__wbg_ptr, modified.__wbg_ptr, ignore_whitespace, budget_ms);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Edits made after `version`, encoded for a language module (see [`EditBatch`]).
     * @param {number} version
     * @returns {EditBatch}
     */
    editsSince(version) {
        const ret = wasm.model_editsSince(this.__wbg_ptr, version);
        return EditBatch.__wrap(ret);
    }
    /**
     * The text between two char offsets.
     * @param {number} start
     * @param {number} end
     * @returns {string}
     */
    getTextRange(start, end) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.model_getTextRange(retptr, this.__wbg_ptr, start, end);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {string}
     */
    getValue() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.model_getValue(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Ids of every language the editor knows.
     * @returns {string[]}
     */
    static knownLanguages() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.model_knownLanguages(retptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayJsValueFromWasm0(r0, r1);
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The language module (pack) that parses `language`, or `""` for an unknown id.
     * @param {string} language
     * @returns {string}
     */
    static languagePack(language) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const ptr0 = passStringToWasm0(language, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.model_languagePack(retptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    lineCount() {
        const ret = wasm.model_lineCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} text
     */
    constructor(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.model_new(ptr0, len0);
        this.__wbg_ptr = ret;
        ModelFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * The char offset of a line and column (0-based), clamped into the text.
     * @param {number} line
     * @param {number} column
     * @returns {number}
     */
    offsetAt(line, column) {
        const ret = wasm.model_offsetAt(this.__wbg_ptr, line, column);
        return ret >>> 0;
    }
    /**
     * Line and column (0-based, in chars) of a char offset.
     * @param {number} offset
     * @returns {Uint32Array}
     */
    positionAt(offset) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            wasm.model_positionAt(retptr, this.__wbg_ptr, offset);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Rows for a list of references: `ranges` holds start and end char offsets per range.
     * Answers JSON, sorted by start: `[{ start, end, line, pre, match, post }]`, where `pre`
     * is the line before the match without its indentation and `post` the rest of the line.
     * @param {Uint32Array | number[]} ranges
     * @returns {string}
     */
    referenceRows(ranges) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const ptr0 = passArray32ToWasm0(ranges, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.model_referenceRows(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Replaces the decorations of `owner` (an id the caller picks, at or above 2^20 so it
     * stays clear of editors' own) with triples of start, end (char offsets) and kind from
     * [`define_decoration_type`]. Every editor showing the model draws them; they move with
     * edits.
     * @param {number} owner
     * @param {Uint32Array | number[]} triples
     */
    setDecorations(owner, triples) {
        const val2 = +owner;
        const ptr0 = passArray32ToWasm0(triples, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.model_setDecorations(this.__wbg_ptr, val2, ptr0, len0);
    }
    /**
     * Sets comment tokens and auto-closing pairs from the language catalog; `""` clears them.
     * @param {string} language
     */
    setLanguageConfig(language) {
        const ptr0 = passStringToWasm0(language, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.model_setLanguageConfig(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Replaces the text and clears the undo history.
     * @param {string} text
     */
    setValue(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.model_setValue(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * An outline tree from a language module's flat symbols (six numbers each: start, end,
     * name start, name end, kind, depth), as JSON:
     * `[{ name, kind, range: { start, end }, selectionRange, children }]`.
     * @param {Uint32Array | number[]} flat
     * @returns {string}
     */
    symbolTree(flat) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const ptr0 = passArray32ToWasm0(flat, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.model_symbolTree(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Increases with every text change; the JS side uses it to fire change events.
     * @returns {number}
     */
    version() {
        const ret = wasm.model_version(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) Model.prototype[Symbol.dispose] = Model.prototype.free;

/**
 * A copy of a model's text that follows its edits, for work in a worker (search).
 */
export class TextReplica {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        TextReplicaFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_textreplica_free(ptr, 0);
    }
    /**
     * Diffs this text (the original) against `modified`. The result is flat: a completeness
     * flag, the change count, then per change the old and new line ranges, the moved partner
     * plus one, the char change count and four offsets per char change.
     * After `budget_ms` the rest is diffed coarsely.
     * @param {TextReplica} modified
     * @param {boolean} ignore_whitespace
     * @param {number} budget_ms
     * @returns {Uint32Array}
     */
    diff(modified, ignore_whitespace, budget_ms) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            _assertClass(modified, TextReplica);
            wasm.textreplica_diff(retptr, this.__wbg_ptr, modified.__wbg_ptr, ignore_whitespace, budget_ms);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export3(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Applies an edit batch as [`EditBatch`] numbers it: nine numbers per edit (start, old
     * end and new end in bytes, then points), the inserted texts concatenated.
     * @param {Uint32Array | number[]} numbers
     * @param {string} text
     */
    edit(numbers, text) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            let ptr0, len0, ptr1, len1;
            try {
                ptr0 = passArray32ToWasm0(numbers, wasm.__wbindgen_export);
                len0 = WASM_VECTOR_LEN;
                ptr1 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
                len1 = WASM_VECTOR_LEN;
            } catch (e) {
                if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
                if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 4, 4); }
                throw e;
            }
            wasm.textreplica_edit(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeFromExternrefTable0(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Every match of the query as pairs of start and end char offsets, or undefined when the
     * regular expression does not compile.
     * @param {string} pattern
     * @param {boolean} regex
     * @param {boolean} case_sensitive
     * @param {boolean} whole_word
     * @returns {Uint32Array | undefined}
     */
    findAll(pattern, regex, case_sensitive, whole_word) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
            const val2 = +regex;
            const val3 = +case_sensitive;
            const val4 = +whole_word;
            const ptr0 = passStringToWasm0(pattern, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.textreplica_findAll(retptr, this.__wbg_ptr, ptr0, len0, val2, val3, val4);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            let v5;
            if (r0 !== 0) {
                v5 = getArrayU32FromWasm0(r0, r1).slice();
                wasm.__wbindgen_export3(r0, r1 * 4, 4);
            }
            return v5;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @param {string} text
     */
    constructor(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.textreplica_new(ptr0, len0);
        this.__wbg_ptr = ret;
        TextReplicaFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @param {string} text
     */
    reset(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.textreplica_reset(this.__wbg_ptr, ptr0, len0);
    }
}
if (Symbol.dispose) TextReplica.prototype[Symbol.dispose] = TextReplica.prototype.free;

/**
 * Defines how decorations of a new kind draw and returns the kind. Empty strings leave a
 * part out. `underline` is `"solid"`, `"wavy"`, `"dotted"` or empty. Types are shared by
 * every editor.
 * @param {string} background
 * @param {string} underline
 * @param {string} underline_color
 * @param {boolean} whole_line
 * @param {string} gutter
 * @param {string} overview
 * @returns {number}
 */
export function defineDecorationType(background, underline, underline_color, whole_line, gutter, overview) {
    const val6 = +whole_line;
    let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4;
    try {
        ptr0 = passStringToWasm0(background, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        len0 = WASM_VECTOR_LEN;
        ptr1 = passStringToWasm0(underline, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        len1 = WASM_VECTOR_LEN;
        ptr2 = passStringToWasm0(underline_color, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        len2 = WASM_VECTOR_LEN;
        ptr3 = passStringToWasm0(gutter, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        len3 = WASM_VECTOR_LEN;
        ptr4 = passStringToWasm0(overview, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        len4 = WASM_VECTOR_LEN;
    } catch (e) {
        if (ptr4) { wasm.__wbindgen_export3(ptr4, len4 * 1, 1); }
        if (ptr3) { wasm.__wbindgen_export3(ptr3, len3 * 1, 1); }
        if (ptr2) { wasm.__wbindgen_export3(ptr2, len2 * 1, 1); }
        if (ptr1) { wasm.__wbindgen_export3(ptr1, len1 * 1, 1); }
        if (ptr0) { wasm.__wbindgen_export3(ptr0, len0 * 1, 1); }
        throw e;
    }
    const ret = wasm.defineDecorationType(ptr0, len0, ptr1, len1, ptr2, len2, val6, ptr3, len3, ptr4, len4);
    return ret >>> 0;
}

/**
 * Filler blocks for a side-by-side diff (see `rustaco_core::diff_view::fillers`), three
 * numbers each: 1 for the modified pane or 0 for the original, the line the block follows
 * (-1: before the first) and its height in lines.
 * @param {Uint32Array | number[]} flat
 * @returns {Int32Array}
 */
export function diffFillers(flat) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
        const ptr0 = passArray32ToWasm0(flat, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.diffFillers(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v2 = getArrayI32FromWasm0(r0, r1).slice();
        wasm.__wbindgen_export3(r0, r1 * 4, 4);
        return v2;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * The first line of the next (or previous) change around `line` of the modified text, or
 * -1 without changes.
 * @param {Uint32Array | number[]} flat
 * @param {number} line
 * @param {boolean} forward
 * @returns {number}
 */
export function diffNextChange(flat, line, forward) {
    const val2 = +line;
    const val3 = +forward;
    const ptr0 = passArray32ToWasm0(flat, wasm.__wbindgen_export);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.diffNextChange(ptr0, len0, val2, val3);
    return ret;
}

/**
 * Unchanged stretches to fold on one side, as pairs of first and last line.
 * @param {Uint32Array | number[]} flat
 * @param {boolean} in_modified
 * @param {number} line_count
 * @param {number} context
 * @returns {Uint32Array}
 */
export function diffUnchangedRegions(flat, in_modified, line_count, context) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
        const val2 = +in_modified;
        const val3 = +line_count;
        const val4 = +context;
        const ptr0 = passArray32ToWasm0(flat, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.diffUnchangedRegions(retptr, ptr0, len0, val2, val3, val4);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v5 = getArrayU32FromWasm0(r0, r1).slice();
        wasm.__wbindgen_export3(r0, r1 * 4, 4);
        return v5;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * Hover and documentation markdown as blocks (see `rustaco_view::markdown`), as JSON:
 * `[["pre", text] | ["p", [[kind, text], ...]]]` with kind `text`, `code`, `b` or `i`.
 * @param {string} text
 * @returns {string}
 */
export function parseMarkdown(text) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.parseMarkdown(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Selects a bundled theme: `"dark"`, `"light"` or `"high-contrast"`.
 * @param {string} name
 * @returns {boolean}
 */
export function setBuiltinTheme(name) {
    const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.setBuiltinTheme(ptr0, len0);
    return ret !== 0;
}

/**
 * Sets a VS Code colour theme from its JSON text (see `rustaco_syntax::Theme::from_vscode`).
 * @param {string} json
 */
export function setVsCodeTheme(json) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
        const ptr0 = passStringToWasm0(json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.setVsCodeTheme(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        if (r1) {
            throw takeFromExternrefTable0(r0);
        }
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * CSS colours of every style id under the current theme, for DOM that shows highlighted
 * text (the deleted lines of an inline diff).
 * @returns {string[]}
 */
export function styleColors() {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
        wasm.styleColors(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayJsValueFromWasm0(r0, r1);
        wasm.__wbindgen_export3(r0, r1 * 4, 4);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * The style id of a highlight name (`"function"`, `"variable.parameter"`, ...), for
 * semantic tokens; names without a style get the closest broader one, or 0.
 * @param {string} name
 * @returns {number}
 */
export function styleId(name) {
    const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.styleId(ptr0, len0);
    return ret >>> 0;
}

/**
 * Colours for DOM widgets drawn over the editor (the find widget, a host's hover boxes):
 * background, foreground, border, input background and the find match colour, as CSS.
 * @returns {string[]}
 */
export function widgetColors() {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16) >>> 0;
        wasm.widgetColors(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayJsValueFromWasm0(r0, r1);
        wasm.__wbindgen_export3(r0, r1 * 4, 4);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_string_get_7ce8eee7fd327545: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_4c88d8bf264fd984: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_beginPath_ab690dc0c7efe6dc: function(arg0) {
            arg0.beginPath();
        },
        __wbg_clientHeight_b40e8971aed9fa2b: function(arg0) {
            const ret = arg0.clientHeight;
            return ret;
        },
        __wbg_clientWidth_269ea87f17b863bf: function(arg0) {
            const ret = arg0.clientWidth;
            return ret;
        },
        __wbg_clip_26239db7c0b67972: function(arg0) {
            arg0.clip();
        },
        __wbg_closePath_e9e11e034c5d75a3: function(arg0) {
            arg0.closePath();
        },
        __wbg_draw_blocks_c3b20cfedb7b2b45: function(arg0, arg1, arg2, arg3, arg4) {
            draw_blocks(arg0, getArrayF64FromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        },
        __wbg_draw_runs_31531a007fdd9b4c: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8) {
            draw_runs(arg0, getStringFromWasm0(arg1, arg2), getArrayF64FromWasm0(arg3, arg4), getStringFromWasm0(arg5, arg6), getStringFromWasm0(arg7, arg8));
        },
        __wbg_fillRect_e1f0b9915220348b: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.fillRect(arg1, arg2, arg3, arg4);
        },
        __wbg_fillText_89d353a9ad9f4a1a: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
        }, arguments); },
        __wbg_fill_cb36fe21a8d46cd7: function(arg0) {
            arg0.fill();
        },
        __wbg_getContext_d93f904b714852ae: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getDate_a44b5f4a8c42a3c3: function(arg0) {
            const ret = arg0.getDate();
            return ret;
        },
        __wbg_getFullYear_94114da14f235fca: function(arg0) {
            const ret = arg0.getFullYear();
            return ret;
        },
        __wbg_getHours_d55d13f9f95415bb: function(arg0) {
            const ret = arg0.getHours();
            return ret;
        },
        __wbg_getMinutes_d0d71c490ef83b47: function(arg0) {
            const ret = arg0.getMinutes();
            return ret;
        },
        __wbg_getMonth_a4698d688e95d9bd: function(arg0) {
            const ret = arg0.getMonth();
            return ret;
        },
        __wbg_getSeconds_7143dabeecfaf7d1: function(arg0) {
            const ret = arg0.getSeconds();
            return ret;
        },
        __wbg_instanceof_CanvasRenderingContext2d_72a60be8aaadecab: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_lineTo_91262b25fb37c461: function(arg0, arg1, arg2) {
            arg0.lineTo(arg1, arg2);
        },
        __wbg_measureText_d831259b682a3f7d: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.measureText(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_moveTo_b4cd3b0b7e1585a5: function(arg0, arg1, arg2) {
            arg0.moveTo(arg1, arg2);
        },
        __wbg_new_0_36d738a0c8cfac43: function() {
            const ret = new Date();
            return ret;
        },
        __wbg_new_37eb45aaf5eeb820: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_now_463ac581d2fc7ea3: function() {
            const ret = Date.now();
            return ret;
        },
        __wbg_push_c9f4893fc209f768: function(arg0, arg1) {
            const ret = arg0.push(arg1);
            return ret;
        },
        __wbg_random_df36b90cd78a7d54: function() {
            const ret = Math.random();
            return ret;
        },
        __wbg_rect_ab5601e64afdbc33: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.rect(arg1, arg2, arg3, arg4);
        },
        __wbg_restore_f322d9dcee2d3472: function(arg0) {
            arg0.restore();
        },
        __wbg_save_1375b4c2f3596b8b: function(arg0) {
            arg0.save();
        },
        __wbg_setTransform_ee8f5d076fcdec0f: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setTransform(arg1, arg2, arg3, arg4, arg5, arg6);
        }, arguments); },
        __wbg_set_fillStyle_95fde67582a9c695: function(arg0, arg1, arg2) {
            arg0.fillStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_font_aa0c0c514c73a5fa: function(arg0, arg1, arg2) {
            arg0.font = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_globalAlpha_4bdefe001e9c79ce: function(arg0, arg1) {
            arg0.globalAlpha = arg1;
        },
        __wbg_set_height_ae0610d318c3428c: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_lineWidth_12f039d09cd36dad: function(arg0, arg1) {
            arg0.lineWidth = arg1;
        },
        __wbg_set_strokeStyle_b90c8a6e0d37c80d: function(arg0, arg1, arg2) {
            arg0.strokeStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_textAlign_0a32c51401892c11: function(arg0, arg1, arg2) {
            arg0.textAlign = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_textBaseline_ff2e3c5c84684b23: function(arg0, arg1, arg2) {
            arg0.textBaseline = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_width_5f89a551b5472887: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_stroke_19fa3f3ec093bc25: function(arg0) {
            arg0.stroke();
        },
        __wbg_width_28f5533f1b3b5abc: function(arg0) {
            const ret = arg0.width;
            return ret;
        },
        __wbindgen_generic_0000000000000001: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            table.set(0, undefined);
            table.set(1024, undefined);
            table.set(1025, null);
            table.set(1026, true);
            table.set(1027, false);
        },
    };
    return {
        __proto__: null,
        "./rustaco_web_bg.js": import0,
    };
}

const EditBatchFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_editbatch_free(ptr, 1));
const EditorFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_editor_free(ptr, 1));
const LspSessionFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_lspsession_free(ptr, 1));
const ModelFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_model_free(ptr, 1));
const TextReplicaFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_textreplica_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__wbindgen_export5();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}

function getArrayF64FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat64ArrayMemory0().subarray(ptr / 8, ptr / 8 + len);
}

function getArrayI32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getInt32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getDataViewMemory0();
    const result = [];
    for (let i = ptr; i < ptr + 4 * len; i += 4) {
        result.push(wasm.__wbindgen_externrefs.get(mem.getUint32(i, true)));
    }
    wasm.__wbindgen_export6(ptr, len);
    return result;
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat64ArrayMemory0 = null;
function getFloat64ArrayMemory0() {
    if (cachedFloat64ArrayMemory0 === null || cachedFloat64ArrayMemory0.byteLength === 0) {
        cachedFloat64ArrayMemory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64ArrayMemory0;
}

let cachedInt32ArrayMemory0 = null;
function getInt32ArrayMemory0() {
    if (cachedInt32ArrayMemory0 === null || cachedInt32ArrayMemory0.byteLength === 0) {
        cachedInt32ArrayMemory0 = new Int32Array(wasm.memory.buffer);
    }
    return cachedInt32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_export4(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayJsValueToWasm0(array, malloc) {
    const ptr = malloc(array.length * 4, 4) >>> 0;
    for (let i = 0; i < array.length; i++) {
        const add = addToExternrefTable0(array[i]);
        getDataViewMemory0().setUint32(ptr + 4 * i, add, true);
    }
    WASM_VECTOR_LEN = array.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__wbindgen_export7(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(bytes) {
    numBytesDecoded += bytes.length;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true });
        cachedTextDecoder.decode();
        numBytesDecoded = bytes.length;
    }
    return cachedTextDecoder.decode(bytes);
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat64ArrayMemory0 = null;
    cachedInt32ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

let __wbg_init_promise;
async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;
    if (__wbg_init_promise === undefined) {
        __wbg_init_promise = __wbg_init_once(module_or_path);
        __wbg_init_promise.catch(() => { __wbg_init_promise = undefined; });
    }
    return __wbg_init_promise;
}

async function __wbg_init_once(module_or_path) {

    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('rustaco_web_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
