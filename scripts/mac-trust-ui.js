ObjC.import('Foundation');
ObjC.import('stdlib');
ObjC.import('ApplicationServices');
ObjC.import('AppKit');

let stage = 1;
let agentPID = null;
function fail(code) {
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
function agentWindows() {
    const agents = $.NSRunningApplication.runningApplicationsWithBundleIdentifier('com.apple.SecurityAgent');
    if (Number(agents.count) > 1) fail(65);
    if (Number(agents.count) === 0) return [];
    const pid = Number(agents.objectAtIndex(0).processIdentifier);
    if (agentPID !== null && agentPID !== pid) fail(65);
    agentPID = pid;
    const app = $.AXUIElementCreateApplication(pid);
    if (Number($.AXUIElementSetMessagingTimeout(app, 0.2)) !== 0) fail(66);
    return children(app, 'AXWindows');
}
function inspect(window) {
    const pending = [{ element: window, depth: 0 }];
    const result = { text: [], users: [], passwords: [], buttons: [] };
    let count = 0;
    while (pending.length) {
        const entry = pending.pop();
        if (++count > 160 || entry.depth > 8) fail(66);
        const element = entry.element;
        const role = text(element, 'AXRole');
        if (role === 'AXStaticText') result.text.push(text(element, 'AXValue'));
        if (role === 'AXTextField') {
            if (text(element, 'AXSubrole', true) === 'AXSecureTextField') result.passwords.push(element);
            else result.users.push(element);
        }
        if (role === 'AXButton' && ObjC.unwrap(attribute(element, 'AXEnabled'))) result.buttons.push(element);
        for (const child of children(element, 'AXChildren')) pending.push({ element: child, depth: entry.depth + 1 });
    }
    return result;
}
function run() {
    try {
        const data = $.NSFileHandle.fileHandleWithStandardInput.readDataOfLength(4097);
        if (Number(data.length) > 4096) fail(64);
        const decoded = $.NSString.alloc.initWithDataEncoding(data, $.NSUTF8StringEncoding);
        if (!decoded) fail(64);
        const input = JSON.parse(ObjC.unwrap(decoded));
        if (Object.keys(input).sort().join(',') !== 'marker,name,password' ||
            typeof input.marker !== 'string' || !/^mac-trust-[0-9a-f]{32}$/.test(input.marker) ||
            typeof input.name !== 'string' || !/^_agensitls[0-9a-f]{16}$/.test(input.name) ||
            typeof input.password !== 'string' || !/^[0-9a-f]{64}$/.test(input.password)) fail(64);
        stage = 2;
        if (!$.AXIsProcessTrusted()) fail(65);
        stage = 3;
        if (agentWindows().length !== 0) fail(65);
        stage = 4;
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('R\n').dataUsingEncoding($.NSUTF8StringEncoding));
        const deadline = Date.now() + 4000;
        while (Date.now() < deadline) {
            let controls;
            let approve;
            try {
                const windows = agentWindows();
                if (windows.length > 1) fail(65);
                if (windows.length === 1) {
                    controls = inspect(windows[0]);
                    if (!controls.text.some(value => value.indexOf(input.marker) !== -1)) fail(67);
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
                if (Number($.AXUIElementSetAttributeValue(controls.passwords[0], $('AXValue'), $(input.password))) !== 0) fail(68);
                input.password = '';
                if (Number($.AXUIElementPerformAction(approve[0], $('AXPress'))) !== 0) fail(68);
                return;
            }
            delay(0.05);
        }
        fail(69);
    } catch (error) {
        const code = Number(error.errorNumber);
        fail(Number.isInteger(code) && Math.abs(code) <= 1000000 ? code : 70);
    }
}
