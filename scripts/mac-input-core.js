ObjC.import('Foundation');
ObjC.import('stdlib');
ObjC.import('ApplicationServices');
ObjC.import('AppKit');
ObjC.import('Carbon');
ObjC.bindFunction('CFRelease', ['void', ['void *']]);
ObjC.bindFunction('getppid', ['int', []]);

return function nativeInputWorker(fixtureMode) {
let fixtureMarker = null;
let stage = 1;
let agentPID = null;
let agentApplication = null;
let pairedInput = false;
let focusFailure = 0;
let focusSeen = 0;
let focusSnapshot = 0;
let timing = null;
let passwordPhase = false;
let timingSent = false;
function uptime() {
    const value = Number($.NSProcessInfo.processInfo.systemUptime);
    if (!Number.isFinite(value)) throw { errorNumber: 93 };
    return value;
}
function elapsed() { return (uptime() - timing.start) * 1000; }
function emitTiming() {
    if (!timing || timingSent) return;
    timingSent = true;
    const values = [timing.tab, timing.secure, timing.first, timing.count, timing.total,
        timing.createDown, timing.createUp, timing.flags, timing.post];
    const encoded = values.map((value, index) => {
        if (!Number.isFinite(value) || value < 0) throw { errorNumber: 93 };
        return Math.min(index === 3 ? 999 : 9999, Math.floor(value));
    });
    $.NSFileHandle.fileHandleWithStandardOutput.writeData(
        $('T' + encoded.join(':') + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
}
function fail(code) {
    if ((code === 76 || code === 77) && focusFailure) code = focusFailure;
    if (pairedInput) throw { errorNumber: code };
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
function focusAttributes(app) {
    const values = Ref();
    const status = Number($.AXUIElementCopyMultipleAttributeValues(
        app, $(['AXWindows', 'AXFocusedWindow', 'AXFocusedUIElement']), 1, values));
    if (status !== 0) fail(status);
    const result = ObjC.castRefToObject(values[0]);
    if (!result || Number($.CFGetTypeID(result)) !== Number($.CFArrayGetTypeID()) ||
        Number(result.count) !== 3) fail(66);
    const windows = result.objectAtIndex(0);
    if (Number($.CFGetTypeID(windows)) !== Number($.CFArrayGetTypeID())) fail(66);
    const count = Number(windows.count);
    if (!Number.isInteger(count) || count < 0 || count > 160) fail(66);
    const elementType = Number($.AXUIElementGetTypeID());
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
function focused(window, secure, nonce, marker, deadline) {
    const measured = fixtureMode && passwordPhase;
    const started = measured ? uptime() : 0;
    try {
        if (Date.now() >= deadline) { if (!focusFailure) focusFailure = 85; return false; }
        const keyboard = usKeyboard();
        if (!keyboard) { focusFailure = 86; return false; }
        const app = agentElement();
        if (app === null) { focusFailure = 87; return false; }
        const attributes = focusAttributes(app);
        const windows = attributes.objectAtIndex(0);
        if (Number(windows.count) !== 1 || !$.CFEqual(windows.objectAtIndex(0), window)) { focusFailure = 87; return false; }
        const matchingNonce = text(nonce, 'AXValue').indexOf(marker) !== -1;
        if (!matchingNonce) { focusFailure = 88; return false; }
        if (!$.CFEqual(attributes.objectAtIndex(1), window)) { focusFailure = 89; return false; }
        if (!$.CFEqual(attributes.objectAtIndex(2), secure)) { focusFailure = 90; return false; }
        focusFailure = 0;
        return true;
    } finally {
        if (measured) {
            timing.count++;
            timing.total += (uptime() - started) * 1000;
        }
    }
}
function enterPassword(window, username, secure, nonce, input, deadline) {
    ObjC.bindFunction('CGEventPostToPid', ['void', ['int', 'void *']]);
    if (!usKeyboard()) fail(83);
    while (true) {
        if (Date.now() >= deadline) fail(91);
        if (!usKeyboard()) fail(83);
        const windows = agentWindows();
        if (windows.length !== 1 || !$.CFEqual(windows[0], window)) fail(65);
        if (text(nonce, 'AXValue').indexOf(input.marker) === -1) fail(67);
        const app = $.AXUIElementCreateApplication(agentPID);
        if (Number($.AXUIElementSetMessagingTimeout(app, 0.2)) !== 0) fail(66);
        if (ObjC.unwrap(attribute(app, 'AXFrontmost')) === true &&
            $.CFEqual(attribute(app, 'AXFocusedWindow'), window)) break;
        delay(0.01);
    }
    if (Date.now() >= deadline) fail(91);
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
    if (timing) timing.tab = elapsed();
    while (!focused(window, secure, nonce, input.marker, deadline)) {
        if (focusFailure === 87) focusSeen |= 1;
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
        if (Date.now() >= deadline) fail(76);
        delay(0.01);
    }
    if (timing) timing.secure = elapsed();
    passwordPhase = fixtureMode;
    // Physical ANSI-US positions; secure input may ignore Unicode overrides.
    const keys = { '0': 0x1d, '1': 0x12, '2': 0x13, '3': 0x14, '4': 0x15,
        '5': 0x17, '6': 0x16, '7': 0x1a, '8': 0x1c, '9': 0x19,
        a: 0x00, b: 0x0b, c: 0x08, d: 0x02, e: 0x0e, f: 0x03 };
    if (timing) timing.first = elapsed();
    for (let index = 0; index < input.password.length; index++) {
        const key = keys[input.password[index]];
        if (key === undefined) fail(64);
        let operationStart = timing ? uptime() : 0;
        const down = $.CGEventCreateKeyboardEvent(source, key, true);
        if (timing) timing.createDown += (uptime() - operationStart) * 1000;
        operationStart = timing ? uptime() : 0;
        const up = $.CGEventCreateKeyboardEvent(source, key, false);
        if (timing) timing.createUp += (uptime() - operationStart) * 1000;
        if (!down || !up) fail(76);
        operationStart = timing ? uptime() : 0;
        $.CGEventSetFlags(down, 0);
        $.CGEventSetFlags(up, 0);
        if (timing) timing.flags += (uptime() - operationStart) * 1000;
        let sent = false;
        let released = false;
        pairedInput = true;
        try {
            if (!focused(window, secure, nonce, input.marker, deadline)) fail(76);
            operationStart = timing ? uptime() : 0;
            $.CGEventPostToPid(agentPID, down);
            sent = true;
            if (timing) timing.post += (uptime() - operationStart) * 1000;
            if (!focused(window, secure, nonce, input.marker, deadline)) fail(77);
            operationStart = timing ? uptime() : 0;
            $.CGEventPostToPid(agentPID, up);
            released = true;
            if (timing) timing.post += (uptime() - operationStart) * 1000;
        } finally {
            pairedInput = false;
            if (sent && !released && agentApplication && !agentApplication.terminated &&
                Number(agentApplication.processIdentifier) === agentPID) {
                try {
                    operationStart = timing ? uptime() : 0;
                } finally {
                    $.CGEventPostToPid(agentPID, up);
                }
                if (timing) timing.post += (uptime() - operationStart) * 1000;
            }
        }
    }
    passwordPhase = false;
    return source;
}
function run() {
    try {
        const data = $.NSFileHandle.fileHandleWithStandardInput.readDataOfLength(4097);
        if (Number(data.length) > 4096) fail(64);
        const decoded = $.NSString.alloc.initWithDataEncoding(data, $.NSUTF8StringEncoding);
        if (!decoded) fail(64);
        const input = JSON.parse(ObjC.unwrap(decoded));
        if (Object.keys(input).sort().join(',') !== (fixtureMode ? 'fixturePID,marker,name,password' : 'marker,name,password') ||
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
        stage = 2;
        if (!$.AXIsProcessTrusted()) fail(65);
        ObjC.bindFunction('CGPreflightPostEventAccess', ['bool', []]);
        if (!$.CGPreflightPostEventAccess()) fail(84);
        stage = 3;
        if (!fixtureMode && agentWindows().length !== 0) fail(65);
        stage = 4;
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('R\n').dataUsingEncoding($.NSUTF8StringEncoding));
        const deadline = Date.now() + 4000;
        if (fixtureMode) timing = { start: uptime(), tab: 9999, secure: 9999, first: 9999,
            count: 0, total: 0, createDown: 0, createUp: 0, flags: 0, post: 0 };
        while (Date.now() < deadline) {
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
                    $.CGEventPostToPid(agentPID, down);
                    submitted = true;
                } finally {
                    // Submission can dismiss the prompt; release only to the original live process.
                    if (submitted && agentApplication && !agentApplication.terminated &&
                        Number(agentApplication.processIdentifier) === agentPID) $.CGEventPostToPid(agentPID, up);
                }
                while (Date.now() < deadline) {
                    const remaining = agentWindows();
                    if (remaining.length === 0) {
                        emitTiming();
                        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('E4:0\n').dataUsingEncoding($.NSUTF8StringEncoding));
                        return;
                    }
                    if (remaining.length !== 1) fail(65);
                    const observed = inspect(remaining[0]);
                    if (!observed.text.some(value => value.indexOf(input.marker) !== -1)) fail(67);
                    if (observed.users.length !== 1 || text(observed.users[0], 'AXValue') !== input.name) fail(79);
                    if (observed.text.some(value => /wrong password|incorrect password|try again|authentication failed/i.test(value))) fail(80);
                    delay(0.05);
                }
                fail(75);
            }
            delay(0.05);
        }
        fail(69);
    } catch (error) {
        const code = Number(error.errorNumber);
        fail(Number.isInteger(code) && Math.abs(code) <= 1000000 ? code : 70);
    }
}
run();
};
