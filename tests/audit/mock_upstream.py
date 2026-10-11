"""A loopback-only TLS/OAuth/MCP fixture; it never forwards traffic to the Internet."""
from __future__ import annotations
import json, logging, pathlib, socket, socketserver, ssl, subprocess, threading, urllib.parse
LOG = logging.getLogger('audit.fixture')

def make_certificates(directory: pathlib.Path) -> tuple[pathlib.Path, ssl.SSLContext]:
    """Create an ephemeral CA without a Python dependency or global trust change."""
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    directory.chmod(0o700)
    ca_path, cert_path, key_path = directory/'ca.pem', directory/'server.pem', directory/'server.key'
    ca_key, request = directory/'ca.key', directory/'server.csr'
    extensions = directory/'extensions.cnf'
    extensions.write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\n'
                          'extendedKeyUsage=serverAuth\nsubjectAltName=DNS:mcp.agensi.io,'
                          'DNS:auth.agensi-audit.test,DNS:ipinfo.io,DNS:api.ipify.org,DNS:bash.ws\n', encoding='ascii')
    def execute(*arguments: str) -> None:
        subprocess.run(['openssl', *map(str, arguments)], check=True, timeout=10,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    execute('req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:prime256v1',
            '-nodes', '-keyout', ca_key, '-out', ca_path, '-days', '2',
            '-subj', '/CN=Ephemeral Agensi audit CA', '-addext', 'basicConstraints=critical,CA:TRUE,pathlen:0',
            '-addext', 'keyUsage=critical,keyCertSign,cRLSign')
    execute('req', '-new', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:prime256v1',
            '-nodes', '-keyout', key_path, '-out', request, '-subj', '/CN=mcp.agensi.io')
    execute('x509', '-req', '-in', request, '-CA', ca_path, '-CAkey', ca_key, '-CAcreateserial',
            '-out', cert_path, '-days', '2', '-sha256', '-extfile', extensions)
    for key in (ca_key, key_path): key.chmod(0o600)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert_path, key_path)
    LOG.debug('Created an isolated TLS fixture; key material omitted.')
    return ca_path, context

class LocalFixture:
    issuer = 'https://auth.agensi-audit.test'
    resource = 'https://mcp.agensi.io/mcp'
    def __init__(self, context: ssl.SSLContext):
        self.context = context
        self.requests = []
        self.connections = []
        self.lock = threading.Lock()
        self.pause_path = None
        self.pause_rpc = None
        self.paused = threading.Event()
        self.resume = threading.Event()
        fixture = self
        class Server(socketserver.ThreadingTCPServer):
            allow_reuse_address = True
            daemon_threads = True
        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                try:
                    fixture.handle(self.request)
                except (OSError, ValueError, ssl.SSLError):
                    LOG.debug('A synthetic connection ended before completion; payload omitted.')
        self.server = Server(('127.0.0.1',0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
    @property
    def port(self): return self.server.server_address[1]
    def start(self): self.thread.start(); return self
    def close(self): self.resume.set(); self.server.shutdown(); self.server.server_close(); self.thread.join(timeout=2)
    @staticmethod
    def exact(sock, length):
        data=b''
        while len(data)<length:
            part=sock.recv(length-len(data))
            if not part: raise OSError('fixture EOF')
            data+=part
        return data
    @staticmethod
    def until(sock, marker, maximum=65536):
        data=b''
        while not data.endswith(marker):
            data += LocalFixture.exact(sock,1)
            if len(data)>maximum: raise ValueError('fixture input bound')
        return data
    def handle(self, sock):
        sock.settimeout(8)
        first=sock.recv(1, socket.MSG_PEEK)
        if first==b'\x04':
            self.exact(sock,8); self.until(sock,b'\x00',512)
            self.until(sock,b'\x00',512)
            protocol='socks4a'
            sock.sendall(b'\x00\x5a\x00\x50\x7f\x00\x00\x01')
        elif first==b'\x05':
            greeting=self.exact(sock,2); self.exact(sock,greeting[1]); sock.sendall(b'\x05\x00')
            header=self.exact(sock,4)
            if header[3]==3: self.exact(sock,self.exact(sock,1)[0])
            elif header[3]==1: self.exact(sock,4)
            elif header[3]==4: self.exact(sock,16)
            else: raise ValueError('invalid address')
            self.exact(sock,2); sock.sendall(b'\x05\x00\x00\x01\x7f\x00\x00\x01\x00\x50')
            protocol='socks5'
        else:
            header=self.until(sock,b'\r\n\r\n')
            if not header.startswith(b'CONNECT '): raise ValueError('CONNECT required')
            protocol='http'
            sock.sendall(b'HTTP/1.1 200 Connection Established\r\n\r\n')
        with self.lock: self.connections.append(protocol)
        tls=self.context.wrap_socket(sock,server_side=True)
        with tls:
            stream=tls.makefile('rb')
            line=stream.readline(65537)
            if len(line)>65536: raise ValueError('request bound')
            parts=line.decode('ascii').strip().split(' ')
            if len(parts)!=3: return
            method,path,_=parts
            headers={}
            for _ in range(100):
                line=stream.readline(65537)
                if line in (b'\r\n',b'\n',b''): break
                key,value=line.decode('latin1').split(':',1); headers[key.lower()]=value.strip()
            body=stream.read(min(int(headers.get('content-length','0')),2*1024*1024))
            with self.lock:
                self.requests.append({'method':method,'path':path,'authenticated':'authorization' in headers,
                    'rpc_method': (json.loads(body).get('method') if body and path == '/mcp' else None)})
            LOG.debug('Synthetic upstream method=%s path=%s', method, urllib.parse.urlsplit(path).path)
            if path==self.pause_path and (self.pause_rpc is None or (body and json.loads(body).get('method')==self.pause_rpc)):
                self.paused.set(); self.resume.wait(6)
            status,data,extra=self.response(method,path,body)
            encoded=json.dumps(data).encode()
            reply=f'HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {len(encoded)}\r\nConnection: close\r\n'
            for key,value in extra.items(): reply+=f'{key}: {value}\r\n'
            tls.sendall(reply.encode()+b'\r\n'+encoded)
    def response(self,method,path,body):
        path=urllib.parse.urlsplit(path).path
        if path in ['/.well-known/oauth-protected-resource/mcp','/.well-known/oauth-protected-resource']:
            return 200,{'resource':self.resource,'authorization_servers':[self.issuer],'scopes_supported':['read','install']},{}
        if path in ['/.well-known/oauth-authorization-server','/.well-known/openid-configuration']:
            return 200,{'issuer':self.issuer,'authorization_endpoint':self.issuer+'/authorize','token_endpoint':self.issuer+'/token',
                'registration_endpoint':self.issuer+'/register','response_types_supported':['code'],
                'grant_types_supported':['authorization_code','refresh_token'],'code_challenge_methods_supported':['S256'],
                'token_endpoint_auth_methods_supported':['none'],'scopes_supported':['read','install'],
                'authorization_response_iss_parameter_supported':True},{}
        if path=='/register':
            request=json.loads(body)
            return 201,{'client_id':'fixture-client','redirect_uris':request['redirect_uris'],'token_endpoint_auth_method':'none',
                'grant_types':['authorization_code','refresh_token'],'response_types':['code']},{}
        if path=='/token':
            return 200,{'access_token':'synthetic-fixture-access','token_type':'Bearer','expires_in':3600,
                'refresh_token':'synthetic-fixture-refresh','scope':'read install'},{}
        if path=='/mcp':
            request=json.loads(body or b'{}'); method=request.get('method'); rid=request.get('id',1)
            if method=='initialize': result={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'audit-fixture','version':'1'}}
            elif method=='tools/list': result={'tools':[{'name':'my_credits','description':'Synthetic read-only fixture','inputSchema':{'type':'object'}}]}
            elif method=='tools/call': result={'content':[{'type':'text','text':'synthetic account fixture'}]}
            else: result={}
            return (202 if method and method.startswith('notifications/') else 200),{'jsonrpc':'2.0','id':rid,'result':result},{}
        return 404,{'error':'fixture not found'},{}
