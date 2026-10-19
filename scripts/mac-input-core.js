ObjC.import('Foundation');
ObjC.import('stdlib');
ObjC.import('ApplicationServices');
$.NSFileHandle.fileHandleWithStandardOutput.writeData($('P3\n').dataUsingEncoding($.NSUTF8StringEncoding));
ObjC.import('AppKit');
$.NSFileHandle.fileHandleWithStandardOutput.writeData($('P4\n').dataUsingEncoding($.NSUTF8StringEncoding));
ObjC.import('Carbon');
ObjC.bindFunction('CFRelease', ['void', ['void *']]);
ObjC.bindFunction('getppid', ['int', []]);
$.NSFileHandle.fileHandleWithStandardOutput.writeData($('P5\n').dataUsingEncoding($.NSUTF8StringEncoding));

return function nativeInputWorker(fixtureMode, channel) {
let fixtureMarker = null;
let stage = 1;
let agentPID = null;
let agentApplication = null;
let agentExecutable = null;
let everValidatedTarget = false;
let postSubmitRead = false;
let pairedInput = false;
let focusFailure = 0;
let focusSeen = 0;
let focusSnapshot = 0;
let timing = null;
let passwordPhase = false;
let timingSent = false;
let windowDiagnosticAttempted = false;
let windowDiagnosticCode = 98;
let cancellationContext = null;
const arrayType = Number($.CFArrayGetTypeID());
const stringType = Number($.CFStringGetTypeID());
const axType = Number($.AXUIElementGetTypeID());
if (![arrayType, stringType, axType].every(value => Number.isFinite(value) && value > 0) ||
    arrayType === stringType || arrayType === axType || stringType === axType) fail(66);
function cancelOwnedPrompt() {
    const context = cancellationContext;
    cancellationContext = null;
    context.input.password = '';
    const deadline = Date.now() + 500;
    function checkTime() { if (Date.now() >= deadline) throw new Error('cleanup-deadline'); }
    function prepare(element) {
        checkTime();
        const remaining = (deadline - Date.now()) / 1000;
        if (remaining <= 0 || Number($.AXUIElementSetMessagingTimeout(element, Math.min(0.2, remaining))) !== 0)
            throw new Error('cleanup-timeout');
        checkTime();
    }
    function read(element, name) {
        prepare(element);
        const value = Ref();
        if (Number($.AXUIElementCopyAttributeValue(element, $(name), value)) !== 0)
            throw new Error('cleanup-read');
        checkTime();
        return ObjC.castRefToObject(value[0]);
    }
    function array(value) {
        if (!value || Number($.CFGetTypeID(value)) !== arrayType ||
            Number(value.count) > 160) throw new Error('cleanup-array');
        return value;
    }
    function boundApplication() {
        checkTime();
        const agents = $.NSRunningApplication.runningApplicationsWithBundleIdentifier('com.apple.SecurityAgent');
        if (Number(agents.count) !== 1 || !agentApplication || agentApplication.terminated)
            throw new Error('cleanup-identity');
        const current = agents.objectAtIndex(0);
        if (Number(current.processIdentifier) !== agentPID || current.terminated || !current.executableURL ||
            String(ObjC.unwrap(current.executableURL.absoluteString)) !== context.executable)
            throw new Error('cleanup-identity');
        const app = $.AXUIElementCreateApplication(agentPID);
        const windows = array(read(app, 'AXWindows'));
        if (Number(windows.count) !== 1 || !$.CFEqual(windows.objectAtIndex(0), context.window))
            throw new Error('cleanup-window');
        if (!$.CFEqual(read(context.nonce, 'AXWindow'), context.window)) throw new Error('cleanup-nonce-window');
        const nonce = read(context.nonce, 'AXValue');
        if (!nonce || Number($.CFGetTypeID(nonce)) !== stringType ||
            String(ObjC.unwrap(nonce)).indexOf(context.input.marker) === -1)
            throw new Error('cleanup-nonce');
        return app;
    }
    try {
        boundApplication();
        const button = read(context.window, 'AXCancelButton');
        if (!button || Number($.CFGetTypeID(button)) !== axType ||
            !$.CFEqual(read(button, 'AXWindow'), context.window) ||
            ObjC.unwrap(read(button, 'AXEnabled')) !== true) throw new Error('cleanup-button');
        prepare(button);
        const actions = Ref();
        if (Number($.AXUIElementCopyActionNames(button, actions)) !== 0) throw new Error('cleanup-actions');
        checkTime();
        const names = array(ObjC.castRefToObject(actions[0]));
        let press = false;
        for (let index = 0; index < Number(names.count); index++) {
            const name = names.objectAtIndex(index);
            if (Number($.CFGetTypeID(name)) !== stringType) throw new Error('cleanup-action-type');
            if (String(ObjC.unwrap(name)) === 'AXPress') press = true;
        }
        if (!press) throw new Error('cleanup-action');
        const app = boundApplication();
        if (!$.CFEqual(read(context.window, 'AXCancelButton'), button) ||
            !$.CFEqual(read(button, 'AXWindow'), context.window) ||
            ObjC.unwrap(read(button, 'AXEnabled')) !== true) throw new Error('cleanup-button-changed');
        prepare(button);
        if (Number($.AXUIElementPerformAction(button, $('AXPress'))) !== 0) throw new Error('cleanup-press');
        checkTime();
        while (true) {
            if (agentApplication.terminated) throw new Error('cleanup-process-ended');
            const windows = array(read(app, 'AXWindows'));
            if (Number(windows.count) === 0) return true;
            if (Number(windows.count) !== 1 || !$.CFEqual(windows.objectAtIndex(0), context.window))
                throw new Error('cleanup-window-changed');
            checkTime();
            delay(Math.min(0.01, Math.max(0, (deadline - Date.now()) / 1000)));
        }
    } catch (_) { return false; }
}
function clockMilliseconds() {
    const value = Date.now();
    if (!Number.isFinite(value)) throw { errorNumber: 93 };
    return value;
}
function elapsed() { return (clockMilliseconds() - timing.start); }
function finalVerificationEnd(inputDeadline, now, eligible) {
    return eligible && now < inputDeadline + 200 ? inputDeadline + 200 : 0;
}
if (finalVerificationEnd(1000, 1000, true) !== 1200 ||
    finalVerificationEnd(1000, 1199, true) !== 1200 ||
    finalVerificationEnd(1000, 1200, true) !== 0 ||
    finalVerificationEnd(1000, 1000, false) !== 0) fail(93);
function emitTiming() {
    if (!timing || timingSent || (!fixtureMode && !everValidatedTarget)) return;
    timingSent = true;
    const values = fixtureMode ? [elapsed(), timing.secure, timing.first, timing.count, timing.total,
        timing.createDown, timing.createUp, timing.flags, timing.post] :
        [elapsed(), timing.secure, timing.count, timing.total];
    const encoded = values.map((value, index) => {
        if (!Number.isFinite(value) || value < 0) throw { errorNumber: 93 };
        return Math.min(index === (fixtureMode ? 3 : 2) ? 999 : 9999, Math.floor(value));
    });
    $.NSFileHandle.fileHandleWithStandardOutput.writeData(
        $((fixtureMode ? 'T' : 'G') + encoded.join(':') + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
}
function fail(code) {
    if ((code === 76 || code === 77) && focusFailure) code = focusFailure;
    if (pairedInput) throw { errorNumber: code };
    if (!fixtureMode && cancellationContext) {
        const canceled = cancelOwnedPrompt();
        $.NSFileHandle.fileHandleWithStandardOutput.writeData(
            $(canceled ? 'C1\n' : 'C0\n').dataUsingEncoding($.NSUTF8StringEncoding));
    }
    emitTiming();
    if (!fixtureMode && focusSeen) $.NSFileHandle.fileHandleWithStandardOutput.writeData(
        $('D' + focusSeen + ':' + focusSnapshot + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
    $.NSFileHandle.fileHandleWithStandardOutput.writeData($('E' + stage + ':' + code + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
    $.exit(code > 0 && code < 128 ? code : 70);
}
function attribute(element, name, optional) {
    const value = Ref();
    const status = Number($.AXUIElementCopyAttributeValue(element, $(name), value));
    if (optional && (status === -25205 || status === -25212)) return null;
    if (status === -25202 && postSubmitRead) throw { invalidPostSubmit: true };
    if (status === -25204 && stage === 4) throw { snapshotPending: true };
    if (status !== 0) fail(status);
    return ObjC.castRefToObject(value[0]);
}
function text(element, name, optional) {
    const value = attribute(element, name, optional);
    return value === null ? '' : String(ObjC.unwrap(value));
}
function children(element, name) {
    const value = attribute(element, name, true);
    if (value === null) return [];
    const count = Number(value.count);
    if (!Number.isInteger(count) || count < 0 || count > 160) fail(66);
    const result = [];
    for (let index = 0; index < count; index++) result.push(value.objectAtIndex(index));
    return result;
}
function agentElement() {
    let target;
    if (fixtureMode) {
        const parent = Number($.getppid());
        if (agentPID !== parent) fail(65);
        target = $.NSRunningApplication.runningApplicationWithProcessIdentifier(parent);
        if (!target || !target.executableURL ||
            String(ObjC.unwrap(target.executableURL.lastPathComponent)) !== fixtureMarker) fail(65);
    } else {
        const agents = $.NSRunningApplication.runningApplicationsWithBundleIdentifier('com.apple.SecurityAgent');
        if (Number(agents.count) > 1) fail(65);
        if (Number(agents.count) === 0) return null;
        target = agents.objectAtIndex(0);
    }
    if (!fixtureMode) {
        if (!target.executableURL) fail(65);
        const executable = String(ObjC.unwrap(target.executableURL.absoluteString));
        if (agentExecutable !== null && executable !== agentExecutable) fail(65);
        agentExecutable = executable;
    }
    const pid = Number(target.processIdentifier);
    if (agentPID !== null && agentPID !== pid) fail(65);
    agentPID = pid;
    if (agentApplication === null) agentApplication = target;
    if (agentApplication.terminated) fail(65);
    const app = $.AXUIElementCreateApplication(pid);
    if (Number($.AXUIElementSetMessagingTimeout(app, 0.2)) !== 0) fail(66);
    return app;
}
function agentWindows() {
    const app = agentElement();
    return app === null ? [] : children(app, 'AXWindows');
}
function boundWindows(deadline) {
    if (Date.now() >= deadline) fail(91);
    const agents = $.NSRunningApplication.runningApplicationsWithBundleIdentifier('com.apple.SecurityAgent');
    if (Number(agents.count) !== 1 || !agentApplication || agentApplication.terminated || agentExecutable === null) fail(65);
    const current = agents.objectAtIndex(0);
    if (Number(current.processIdentifier) !== agentPID || current.terminated || !current.executableURL ||
        String(ObjC.unwrap(current.executableURL.absoluteString)) !== agentExecutable) fail(65);
    const app = $.AXUIElementCreateApplication(agentPID);
    const remaining = (deadline - Date.now()) / 1000;
    if (remaining <= 0 || Number($.AXUIElementSetMessagingTimeout(app, Math.min(0.2, remaining))) !== 0) fail(66);
    const value = Ref();
    const status = Number($.AXUIElementCopyAttributeValue(app, $('AXWindows'), value));
    if (status !== 0) fail(status);
    if (Date.now() >= deadline) fail(91);
    const windows = ObjC.castRefToObject(value[0]);
    if (!windows || Number($.CFGetTypeID(windows)) !== arrayType) fail(66);
    const count = Number(windows.count);
    if (!Number.isInteger(count) || count < 0 || count > 160) fail(66);
    const result = [];
    for (let index = 0; index < count; index++) {
        const window = windows.objectAtIndex(index);
        if (Number($.CFGetTypeID(window)) !== axType) fail(66);
        result.push(window);
    }
    return result;
}
function completedWithoutInput(input, deadline) {
    if (fixtureMode || !input.allowNoPrompt || everValidatedTarget || cancellationContext !== null) return;
    if (!channel.completed()) return;
    input.password = '';
    if (Date.now() >= deadline) fail(91);
    const agents = $.NSRunningApplication.runningApplicationsWithBundleIdentifier('com.apple.SecurityAgent');
    const count = Number(agents.count);
    if (count !== 0 && count !== 1) fail(65);
    if (count === 1 && boundWindows(deadline).length !== 0) fail(65);
    if (Date.now() >= deadline) fail(91);
    $.NSFileHandle.fileHandleWithStandardOutput.writeData($('N2\nE4:0\n').dataUsingEncoding($.NSUTF8StringEncoding));
    $.exit(0);
}
function focusAttributes(app, pendingAllowed) {
    const values = Ref();
    const status = Number($.AXUIElementCopyMultipleAttributeValues(
        app, $(['AXWindows', 'AXFocusedWindow', 'AXFocusedUIElement']), 1, values));
    if (status === -25204 && pendingAllowed) throw { snapshotPending: true };
    if (status !== 0) fail(status);
    const result = ObjC.castRefToObject(values[0]);
    if (!result || Number($.CFGetTypeID(result)) !== arrayType ||
        Number(result.count) !== 3) fail(66);
    const windows = result.objectAtIndex(0);
    if (Number($.CFGetTypeID(windows)) !== arrayType) fail(66);
    const count = Number(windows.count);
    if (!Number.isInteger(count) || count < 0 || count > 160) fail(66);
    const elementType = axType;
    for (let index = 0; index < count; index++) {
        if (Number($.CFGetTypeID(windows.objectAtIndex(index))) !== elementType) fail(66);
    }
    for (let index = 1; index < 3; index++) {
        if (Number($.CFGetTypeID(result.objectAtIndex(index))) !== elementType) fail(66);
    }
    return result;
}
function inspect(window, marker) {
    const pending = [{ element: window, depth: 0 }];
    const result = { window, text: [], nonces: [], users: [], passwords: [], buttons: [] };
    let count = 0;
    while (pending.length) {
        const entry = pending.pop();
        if (++count > 160 || entry.depth > 8) fail(66);
        const element = entry.element;
        const role = text(element, 'AXRole');
        if (role === 'AXStaticText') {
            const value = text(element, 'AXValue');
            result.text.push(value);
            if (marker && value.indexOf(marker) !== -1) result.nonces.push(element);
        }
        if (role === 'AXTextField') {
            if (text(element, 'AXSubrole', true) === 'AXSecureTextField') result.passwords.push(element);
            else result.users.push(element);
        }
        if (role === 'AXButton' && ObjC.unwrap(attribute(element, 'AXEnabled'))) result.buttons.push(element);
        for (const child of children(element, 'AXChildren')) pending.push({ element: child, depth: entry.depth + 1 });
    }
    return result;
}
let keyboardTraced = false;
function usKeyboard() {
    const trace = !keyboardTraced;
    keyboardTraced = true;
    function boundary(number) {
        if (trace && !fixtureMode) $.NSFileHandle.fileHandleWithStandardOutput.writeData(
            $('B' + number + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
    }
    boundary(1);
    const input = $.TISCopyCurrentKeyboardInputSource();
    boundary(2);
    const layout = $.TISCopyCurrentKeyboardLayoutInputSource();
    boundary(3);
    try {
        return Boolean(input && layout) && [input, layout].every((source, index) => {
            boundary(4 + index);
            const identifier = $.TISGetInputSourceProperty(source, $.kTISPropertyInputSourceID);
            return identifier && String(ObjC.unwrap(ObjC.castRefToObject(identifier))) === 'com.apple.keylayout.US';
        });
    } finally {
        boundary(6);
        if (input) $.CFRelease(input);
        boundary(7);
        if (layout) $.CFRelease(layout);
        boundary(8);
    }
}
function focused(window, secure, nonce, marker, deadline, pendingAllowed = false) {
    const measured = passwordPhase;
    const started = measured ? clockMilliseconds() : 0;
    try {
        if (Date.now() >= deadline) { if (!focusFailure) focusFailure = 85; return false; }
        const keyboard = usKeyboard();
        if (!keyboard) { focusFailure = 86; return false; }
        const app = agentElement();
        if (app === null) { focusFailure = 87; return false; }
        const attributes = focusAttributes(app, pendingAllowed);
        const windows = attributes.objectAtIndex(0);
        if (Number(windows.count) === 0) { focusFailure = 94; return false; }
        let nonceValue;
        if (fixtureMode) {
            let matches = 0;
            for (let index = 0; index < Number(windows.count); index++) {
                if ($.CFEqual(windows.objectAtIndex(index), window)) matches++;
            }
            if (matches !== 1) { focusFailure = 96; return false; }
            const owner = attribute(secure, 'AXWindow');
            if (!owner || Number($.CFGetTypeID(owner)) !== axType) fail(66);
            if (!$.CFEqual(owner, window)) { focusFailure = 96; return false; }
            const values = Ref();
            const status = Number($.AXUIElementCopyMultipleAttributeValues(nonce, $(['AXWindow', 'AXValue']), 1, values));
            if (status === -25204 && pendingAllowed) throw { snapshotPending: true };
            if (status !== 0) fail(status);
            const result = ObjC.castRefToObject(values[0]);
            if (!result || Number($.CFGetTypeID(result)) !== arrayType || Number(result.count) !== 2) fail(66);
            const nonceOwner = result.objectAtIndex(0);
            const value = result.objectAtIndex(1);
            if (Number($.CFGetTypeID(nonceOwner)) !== axType ||
                Number($.CFGetTypeID(value)) !== stringType) fail(66);
            if (!$.CFEqual(nonceOwner, window)) { focusFailure = 96; return false; }
            nonceValue = String(ObjC.unwrap(value));
        } else if (Number(windows.count) !== 1) {
            let originalMatches = 0;
            let firstNonoriginal = null;
            for (let index = 0; index < Number(windows.count); index++) {
                const candidate = windows.objectAtIndex(index);
                if ($.CFEqual(candidate, window)) originalMatches++;
                else if (firstNonoriginal === null) firstNonoriginal = candidate;
            }
            focusFailure = originalMatches === Number(windows.count) ? 97 : (originalMatches > 0 ? 98 : 99);
            if (fixtureMode && focusFailure === 98) {
                if (!windowDiagnosticAttempted) {
                    windowDiagnosticAttempted = true;
                    try {
                        if (Date.now() < deadline) {
                            const role = attribute(firstNonoriginal, 'AXRole');
                            if (!role || Number($.CFGetTypeID(role)) !== stringType) fail(66);
                            if (String(ObjC.unwrap(role)) !== 'AXWindow') windowDiagnosticCode = 105;
                            else if (Date.now() < deadline) {
                                const subrole = attribute(firstNonoriginal, 'AXSubrole', true);
                                if (subrole !== null && Number($.CFGetTypeID(subrole)) !== stringType) fail(66);
                                const name = subrole === null ? '' : String(ObjC.unwrap(subrole));
                                const known = ['AXStandardWindow', 'AXFloatingWindow', 'AXDialog', 'AXSystemDialog'].indexOf(name);
                                windowDiagnosticCode = known === -1 ? 104 : 100 + known;
                            }
                        }
                    } catch (error) {
                        if (error.snapshotPending) throw { errorNumber: -25204 };
                        throw error;
                    }
                }
                focusFailure = windowDiagnosticCode;
            }
            return false;
        }
        if (!fixtureMode && !$.CFEqual(windows.objectAtIndex(0), window)) { focusFailure = 96; return false; }
        const matchingNonce = (fixtureMode ? nonceValue : text(nonce, 'AXValue')).indexOf(marker) !== -1;
        if (!matchingNonce) { focusFailure = 88; return false; }
        if (!$.CFEqual(attributes.objectAtIndex(1), window)) { focusFailure = 89; return false; }
        if (!$.CFEqual(attributes.objectAtIndex(2), secure)) { focusFailure = 90; return false; }
        focusFailure = 0;
        return true;
    } finally {
        if (measured) {
            timing.count++;
            timing.total += (clockMilliseconds() - started);
        }
    }
}
function enterPassword(window, username, secure, nonce, input, deadline) {
    ObjC.bindFunction('CGEventPostToPid', ['void', ['int', 'void *']]);
    if (!usKeyboard()) fail(83);
    if (!fixtureMode) {
        function activationTarget() {
            if (Date.now() >= deadline) fail(91);
            if (!usKeyboard()) fail(83);
            const app = agentElement();
            if (app === null) fail(65);
            const windows = children(app, 'AXWindows');
            if (windows.length !== 1 || !$.CFEqual(windows[0], window)) fail(65);
            if (text(nonce, 'AXValue').indexOf(input.marker) === -1) fail(67);
            if (Date.now() >= deadline) fail(91);
            return app;
        }
        if (Number($.AXUIElementSetAttributeValue(activationTarget(), $('AXFrontmost'), $(true))) !== 0) fail(106);
        activationTarget();
        if (Number($.AXUIElementPerformAction(window, $('AXRaise'))) !== 0) fail(107);
    }
    while (true) {
        if (Date.now() >= deadline) fail(91);
        if (!usKeyboard()) fail(83);
        try {
            const windows = agentWindows();
            if (windows.length !== 1 || !$.CFEqual(windows[0], window)) fail(65);
            if (text(nonce, 'AXValue').indexOf(input.marker) === -1) fail(67);
            const app = $.AXUIElementCreateApplication(agentPID);
            if (Number($.AXUIElementSetMessagingTimeout(app, 0.2)) !== 0) fail(66);
            if (ObjC.unwrap(attribute(app, 'AXFrontmost')) === true &&
                $.CFEqual(attribute(app, 'AXFocusedWindow'), window)) break;
        } catch (error) {
            if (!error.snapshotPending) throw error;
        }
        if (Date.now() >= deadline) fail(91);
        delay(0.01);
    }
    if (Date.now() >= deadline) fail(91);
    if (Number($.AXUIElementSetAttributeValue(username, $('AXFocused'), $(true))) !== 0) fail(92);
    const source = $.CGEventSourceCreate(-1);
    if (!source) fail(76);
    const tabDown = $.CGEventCreateKeyboardEvent(source, 0x30, true);
    const tabUp = $.CGEventCreateKeyboardEvent(source, 0x30, false);
    if (!tabDown || !tabUp) fail(76);
    $.CGEventSetFlags(tabDown, 0);
    $.CGEventSetFlags(tabUp, 0);
    if (text(username, 'AXValue') !== input.name ||
        !focused(window, username, nonce, input.marker, deadline)) fail(92);
    let navigated = false;
    try {
        $.CGEventPostToPid(agentPID, tabDown);
        navigated = true;
    } finally {
        if (navigated && agentApplication && !agentApplication.terminated &&
            Number(agentApplication.processIdentifier) === agentPID) $.CGEventPostToPid(agentPID, tabUp);
    }
    while (true) {
        try {
            if (focused(window, secure, nonce, input.marker, deadline, true)) break;
        } catch (error) {
            if (!error.snapshotPending) throw error;
        }
        if (!fixtureMode) {
            if ([87, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105].indexOf(focusFailure) !== -1) focusSeen |= 1;
            if (focusFailure === 89) focusSeen |= 2;
            if (focusFailure === 90) {
                focusSeen |= 4;
                try {
                    const app = $.AXUIElementCreateApplication(agentPID);
                    if (Number($.AXUIElementSetMessagingTimeout(app, 0.2)) !== 0) fail(66);
                    const element = attribute(app, 'AXFocusedUIElement');
                    if (!$.CFEqual(element, username)) {
                        const role = text(element, 'AXRole');
                        const subrole = text(element, 'AXSubrole', true);
                        const owner = attribute(element, 'AXWindow', true);
                        focusSnapshot = 1 |
                            ($.CFEqual(attribute(app, 'AXFocusedWindow'), window) ? 2 : 0) |
                            ($.CFEqual(element, secure) ? 4 : 0) |
                            (role === 'AXTextField' ? 8 : 0) |
                            (role === 'AXTextArea' ? 16 : 0) |
                            (subrole === 'AXSecureTextField' ? 32 : 0) |
                            (owner && $.CFEqual(owner, window) ? 64 : 0) |
                            (owner ? 128 : 0) | (subrole ? 256 : 0);
                    }
                } catch (error) {
                    if (!error.snapshotPending) throw error;
                }
            }
        }
        if (Date.now() >= deadline) fail(76);
        delay(0.01);
    }
    if (timing) timing.secure = elapsed();
    passwordPhase = true;
    // Physical ANSI-US positions; secure input may ignore Unicode overrides.
    const keys = { '0': 0x1d, '1': 0x12, '2': 0x13, '3': 0x14, '4': 0x15,
        '5': 0x17, '6': 0x16, '7': 0x1a, '8': 0x1c, '9': 0x19,
        a: 0x00, b: 0x0b, c: 0x08, d: 0x02, e: 0x0e, f: 0x03 };
    if (timing) timing.first = elapsed();
    for (let index = 0; index < input.password.length; index++) {
        const key = keys[input.password[index]];
        if (key === undefined) fail(64);
        let operationStart = fixtureMode ? clockMilliseconds() : 0;
        const down = $.CGEventCreateKeyboardEvent(source, key, true);
        if (fixtureMode) timing.createDown += (clockMilliseconds() - operationStart);
        operationStart = fixtureMode ? clockMilliseconds() : 0;
        const up = $.CGEventCreateKeyboardEvent(source, key, false);
        if (fixtureMode) timing.createUp += (clockMilliseconds() - operationStart);
        if (!down || !up) fail(76);
        operationStart = fixtureMode ? clockMilliseconds() : 0;
        $.CGEventSetFlags(down, 0);
        $.CGEventSetFlags(up, 0);
        if (fixtureMode) timing.flags += (clockMilliseconds() - operationStart);
        let sent = false;
        let released = false;
        pairedInput = true;
        try {
            if (!focused(window, secure, nonce, input.marker, deadline)) fail(76);
            operationStart = fixtureMode ? clockMilliseconds() : 0;
            $.CGEventPostToPid(agentPID, down);
            sent = true;
            if (fixtureMode) timing.post += (clockMilliseconds() - operationStart);
            if (!focused(window, secure, nonce, input.marker, deadline)) fail(77);
            operationStart = fixtureMode ? clockMilliseconds() : 0;
            $.CGEventPostToPid(agentPID, up);
            released = true;
            if (fixtureMode) timing.post += (clockMilliseconds() - operationStart);
        } finally {
            pairedInput = false;
            if (sent && !released && agentApplication && !agentApplication.terminated &&
                Number(agentApplication.processIdentifier) === agentPID) {
                try {
                    operationStart = fixtureMode ? clockMilliseconds() : 0;
                } finally {
                    $.CGEventPostToPid(agentPID, up);
                }
                if (fixtureMode) timing.post += (clockMilliseconds() - operationStart);
            }
        }
    }
    passwordPhase = false;
    return source;
}
function run() {
    try {
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('P6\n').dataUsingEncoding($.NSUTF8StringEncoding));
        const input = channel.readInput();
        if (Object.keys(input).sort().join(',') !== (fixtureMode ? 'fixturePID,marker,name,password' : 'allowNoPrompt,cancelOnly,marker,name,password') ||
            (!fixtureMode && (typeof input.cancelOnly !== 'boolean' || typeof input.allowNoPrompt !== 'boolean' ||
                (input.cancelOnly && input.allowNoPrompt))) ||
            typeof input.marker !== 'string' || !/^mac-trust-[0-9a-f]{32}$/.test(input.marker) ||
            typeof input.name !== 'string' || !/^_agensitls[0-9a-f]{16}$/.test(input.name) ||
            typeof input.password !== 'string' || !/^[0-9a-f]{64}$/.test(input.password)) fail(64);
        if (fixtureMode) {
            if (!Number.isInteger(input.fixturePID) || input.fixturePID !== Number($.getppid()) ||
                input.name !== '_agensitls0000000000000000' ||
                input.password !== '0123456789abcdef'.repeat(4)) fail(64);
            const parent = $.NSRunningApplication.runningApplicationWithProcessIdentifier(input.fixturePID);
            if (!parent || !parent.executableURL ||
                String(ObjC.unwrap(parent.executableURL.lastPathComponent)) !== input.marker) fail(65);
            agentPID = input.fixturePID;
            agentApplication = parent;
            fixtureMarker = input.marker;
        }
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('P7\n').dataUsingEncoding($.NSUTF8StringEncoding));
        stage = 2;
        if (!$.AXIsProcessTrusted()) fail(65);
        ObjC.bindFunction('CGPreflightPostEventAccess', ['bool', []]);
        if (!$.CGPreflightPostEventAccess()) fail(84);
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('P8\n').dataUsingEncoding($.NSUTF8StringEncoding));
        stage = 3;
        if (!fixtureMode && agentWindows().length !== 0) fail(65);
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('P9\n').dataUsingEncoding($.NSUTF8StringEncoding));
        stage = 4;
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('R\n').dataUsingEncoding($.NSUTF8StringEncoding));
        const deadline = Date.now() + 4000;
        timing = { start: clockMilliseconds(), secure: 9999, first: 9999,
            count: 0, total: 0, createDown: 0, createUp: 0, flags: 0, post: 0 };
        while (Date.now() < deadline) {
            completedWithoutInput(input, deadline);
            let controls;
            let approve;
            try {
                const windows = agentWindows();
                if (windows.length > 1) fail(65);
                if (windows.length === 1) {
                    controls = inspect(windows[0], input.marker);
                    if (controls.nonces.length === 0) fail(67);
                    approve = controls.buttons.filter(button => ['OK', 'Modify Settings', 'Update Settings'].indexOf(text(button, 'AXTitle')) !== -1);
                    if (controls.users.length !== 1 || controls.passwords.length !== 1 || approve.length !== 1) fail(68);
                    if (agentWindows().length !== 1) fail(65);
                }
            } catch (error) {
                if (!error.snapshotPending) throw error;
                controls = null;
                approve = null;
            }
            if (controls && approve) {
                completedWithoutInput(input, deadline);
                everValidatedTarget = true;
                if (!fixtureMode) {
                    if (!agentApplication.executableURL) fail(65);
                    cancellationContext = { window: controls.window, nonce: controls.nonces[0], input,
                        executable: String(ObjC.unwrap(agentApplication.executableURL.absoluteString)) };
                    if (input.cancelOnly) fail(108);
                }
                if (Number($.AXUIElementSetAttributeValue(controls.users[0], $('AXValue'), $(input.name))) !== 0) fail(68);
                if (text(controls.users[0], 'AXValue') !== input.name) fail(74);
                const source = enterPassword(controls.window, controls.users[0], controls.passwords[0], controls.nonces[0], input, deadline);
                input.password = '';
                const down = $.CGEventCreateKeyboardEvent(source, 0x24, true);
                const up = $.CGEventCreateKeyboardEvent(source, 0x24, false);
                if (!down || !up) fail(76);
                $.CGEventSetFlags(down, 0);
                $.CGEventSetFlags(up, 0);
                if (text(controls.users[0], 'AXValue') !== input.name) fail(78);
                if (!focused(controls.window, controls.passwords[0], controls.nonces[0], input.marker, deadline)) fail(76);
                let submitted = false;
                try {
                    if (Date.now() >= deadline) fail(85);
                    $.CGEventPostToPid(agentPID, down);
                    submitted = true;
                } finally {
                    // Submission can dismiss the prompt; release only to the original live process.
                    if (submitted && agentApplication && !agentApplication.terminated &&
                        Number(agentApplication.processIdentifier) === agentPID) $.CGEventPostToPid(agentPID, up);
                }
                while (Date.now() < deadline) {
                    let originalAbsent = false;
                    try {
                        postSubmitRead = !fixtureMode;
                        const remaining = fixtureMode ? agentWindows() : boundWindows(deadline);
                        originalAbsent = fixtureMode
                            ? !remaining.some(candidate => $.CFEqual(candidate, controls.window))
                            : remaining.length === 0;
                        if (!originalAbsent) {
                            if (fixtureMode) {
                                if (remaining.filter(candidate => $.CFEqual(candidate, controls.window)).length !== 1) fail(65);
                            } else if (remaining.length !== 1 || !$.CFEqual(remaining[0], controls.window)) fail(65);
                            const observed = inspect(controls.window);
                            if (!observed.text.some(value => value.indexOf(input.marker) !== -1)) fail(67);
                            if (observed.users.length !== 1 || text(observed.users[0], 'AXValue') !== input.name) fail(79);
                            if (observed.text.some(value => /wrong password|incorrect password|try again|authentication failed/i.test(value))) fail(80);
                        }
                    } catch (error) {
                        postSubmitRead = false;
                        if (!error.invalidPostSubmit) throw error;
                        if (boundWindows(deadline).length !== 0) fail(-25202);
                        originalAbsent = true;
                    } finally { postSubmitRead = false; }
                    if (originalAbsent) {
                        emitTiming();
                        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('E4:0\n').dataUsingEncoding($.NSUTF8StringEncoding));
                        return;
                    }
                    delay(0.05);
                }
                const verificationEnd = finalVerificationEnd(deadline, Date.now(), !fixtureMode && submitted);
                if (verificationEnd && boundWindows(verificationEnd).length === 0) {
                    emitTiming();
                    $.NSFileHandle.fileHandleWithStandardOutput.writeData($('E4:0\n').dataUsingEncoding($.NSUTF8StringEncoding));
                    return;
                }
                fail(75);
            }
            delay(0.05);
        }
        if (!fixtureMode && input.allowNoPrompt && !everValidatedTarget && cancellationContext === null) {
            input.password = '';
            if (boundWindows(Date.now() + 200).length === 0) {
                $.NSFileHandle.fileHandleWithStandardOutput.writeData($('N1\nE4:69\n').dataUsingEncoding($.NSUTF8StringEncoding));
                $.exit(69);
            }
        }
        fail(69);
    } catch (error) {
        const code = Number(error.errorNumber);
        fail(Number.isInteger(code) && Math.abs(code) <= 1000000 ? code : 70);
    }
}
run();
};
