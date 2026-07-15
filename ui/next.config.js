const path = require('path');

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',
  // Allow importing the repo-level generated protos (../gen/ts) — used by
  // the BFF's gRPC bridge for tonic-only backends (see app/api/rpc route).
  experimental: { externalDir: true },
  webpack: (config) => {
    // protobuf-es output uses ESM-style `./x_pb.js` imports for .ts files.
    config.resolve.extensionAlias = { '.js': ['.ts', '.js'] };
    // Files under ../gen/ts resolve bare imports (@bufbuild/protobuf)
    // against ui/node_modules, which isn't on their walk-up path.
    config.resolve.modules = [...(config.resolve.modules ?? ['node_modules']), path.resolve(__dirname, 'node_modules')];
    return config;
  },
  // NOTE: no rewrites() here. With standalone output, rewrites() runs at
  // BUILD time and its env-var destinations get frozen into the routes
  // manifest — deployed containers would proxy /api/rpc/* to the build
  // machine's localhost regardless of runtime BACKEND_*_URL values.
  // src/app/api/rpc/[module]/[...rpc]/route.ts handles these paths at
  // runtime instead.
};
module.exports = nextConfig;
