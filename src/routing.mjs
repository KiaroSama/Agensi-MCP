import { Agent, ProxyAgent, buildConnector, fetch, setGlobalDispatcher, Dispatcher } from 'undici';
import { SocksClient } from 'socks';
import { lookup } from 'node:dns/promises';
import { isIP } from 'node:net';
import { validate, proxyUrl } from './config.mjs';

function socksAgent(url, sockets, tls = {}) {
  const tlsConnect = buildConnector({ ...tls, timeout: 10000 });
  const type = url.protocol.startsWith('socks4') ? 4 : 5;
  const localDns = ['socks4:', 'socks5:'].includes(url.protocol);
  return new Agent({ connect: async (options, callback) => {
    let socket;
    try {
      let host = options.hostname.replace(/^\[|\]$/g, '');
      if (localDns && !isIP(host)) host = (await lookup(host, { family: type === 4 ? 4 : 0 })).address;
      const info = await SocksClient.createConnection({
        command: 'connect', timeout: 10000,
        proxy: { host: url.hostname.replace(/^\[|\]$/g, ''), port: Number(url.port), type,
          userId: decodeURIComponent(url.username), password: decodeURIComponent(url.password) },
        destination: { host, port: Number(options.port || (options.protocol === 'https:' ? 443 : 80)) }
      });
      socket = info.socket;
      sockets.add(socket);
      socket.once('close', () => sockets.delete(socket));
      if (options.protocol === 'https:') {
        tlsConnect({ ...options, servername: isIP(options.hostname) ? null : (options.servername || options.hostname),
          httpSocket: socket }, callback);
      } else {
        socket.setNoDelay(true);
        callback(null, socket);
      }
    } catch (error) {
      socket?.destroy();
      callback(error, null);
    }
  } });
}

export function createRoute(config, tls = {}) {
  validate(config);
  const sockets = new Set();
  let dispatcher;
  if (config.mode === 'direct') dispatcher = new Agent({ connect: { timeout: 10000 } });
  if (config.mode === 'proxy') {
    const url = proxyUrl(config.profiles.find(p => p.name === config.selected).url);
    dispatcher = url.protocol.startsWith('socks') ? socksAgent(url, sockets, tls)
      : new ProxyAgent({ uri: url.href, requestTls: tls, proxyTls: tls, connect: { timeout: 10000 } });
  }
  let closed = false;
  return {
    dispatcher,
    dispatch(options, handler) {
      if (closed || !dispatcher) {
        queueMicrotask(() => handler.onError(new Error('Agensi route is disconnected.')));
        return false;
      }
      return dispatcher.dispatch(options, handler);
    },
    fetch(input, options = {}) {
      if (closed || !dispatcher) return Promise.reject(new Error('Agensi route is disconnected.'));
      return fetch(input, { ...options, dispatcher });
    },
    async close() {
      closed = true;
      for (const socket of sockets) socket.destroy();
      await dispatcher?.destroy(new Error('Route changed or closed.'));
    }
  };
}

// The policy object, not an environment agent, owns both SDK and global fetch traffic.
export function installPolicy(config, callbackPort = null, unchanged = () => true) {
  const route = createRoute(config);
  const loopback = new Agent({ connect: { timeout: 1000 } });
  const localCoordination = (origin, path) => {
    const url = new URL(origin);
    return callbackPort && url.protocol === 'http:' && url.hostname === '127.0.0.1' &&
      Number(url.port) === callbackPort && path === '/.mcp-remote/id';
  };
  class PolicyDispatcher extends Dispatcher {
    dispatch(options, handler) {
      if (!unchanged()) {
        route.close().catch(() => {});
        queueMicrotask(() => handler.onError(new Error('Route changed; reconnect Agensi.')));
        return false;
      }
      return localCoordination(options.origin, options.path)
        ? loopback.dispatch(options, handler) : route.dispatch(options, handler);
    }
  }
  const policy = new PolicyDispatcher();
  setGlobalDispatcher(policy);
  globalThis.fetch = (input, options = {}) => fetch(input, { ...options, dispatcher: policy });
  return { route, async close() { await route.close(); await loopback.destroy(); } };
}
