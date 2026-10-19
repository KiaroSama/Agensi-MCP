ObjC.import('Foundation');

return function inputChannel() {
    function reject() { throw { errorNumber: 64 }; }
    function lengthOf(value) {
        if (!/^[0-9]{4}$/.test(value)) reject();
        const length = Number(value);
        if (length < 1 || length > 4096) reject();
        return length;
    }
    function completion(value) {
        if (value !== 'Qw==') reject();
        return true;
    }
    function readable(status, value) {
        if (status === 0 && value === 'AAAAAAEAAAA=') return false;
        if (status === 1 && ['AAAAAAEAAQA=', 'AAAAAAEAEQA='].indexOf(value) !== -1) return true;
        reject();
    }
    function refused(operation) {
        try { operation(); } catch (_) { return; }
        reject();
    }
    if (lengthOf('0001') !== 1 || lengthOf('4096') !== 4096 || !completion('Qw==') ||
        readable(0, 'AAAAAAEAAAA=') || !readable(1, 'AAAAAAEAAQA=') ||
        !readable(1, 'AAAAAAEAEQA=')) reject();
    for (const value of ['0000', '4097', '9999', '001', '00001', ' 001', '０００１'])
        refused(() => lengthOf(value));
    for (const value of ['', 'WA==', 'Q0M=', 'Qwo=']) refused(() => completion(value));
    for (const pair of [[-1, 'AAAAAAEAAAA='], [1, 'AAAAAAEAEAA='], [1, 'AAAAAAEACAA='],
        [1, 'AAAAAAEAIAA='], [0, 'AAAAAAEAAQA='], [2, 'AAAAAAEAAQA='], [1, '']])
        refused(() => readable(pair[0], pair[1]));

    const input = $.NSFileHandle.fileHandleWithStandardInput;
    function readExact(length) {
        const data = input.readDataOfLength(length);
        if (Number(data.length) !== length) reject();
        const decoded = $.NSString.alloc.initWithDataEncoding(data, $.NSUTF8StringEncoding);
        if (!decoded) reject();
        return String(ObjC.unwrap(decoded));
    }
    return {
        readInput() {
            return JSON.parse(readExact(lengthOf(readExact(4))));
        },
        completed() {
            ObjC.bindFunction('poll', ['int', ['void *', 'unsigned int', 'int']]);
            // Darwin arm64/x64 pollfd: fd 0, POLLIN, initially zero revents.
            const descriptor = $.NSData.alloc.initWithBase64EncodedStringOptions($('AAAAAAEAAAA='), 0).mutableCopy;
            const status = Number($.poll(descriptor.mutableBytes, 1, 0));
            const value = String(ObjC.unwrap(descriptor.base64EncodedStringWithOptions(0)));
            if (!readable(status, value)) return false;
            // The parent closes after one byte; the existing owner bounds waiting for EOF.
            return completion(String(ObjC.unwrap(input.readDataOfLength(2).base64EncodedStringWithOptions(0))));
        }
    };
};
