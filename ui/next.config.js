/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',
  // NOTE: no rewrites() here. With standalone output, rewrites() runs at
  // BUILD time and its env-var destinations get frozen into the routes
  // manifest — deployed containers would proxy /api/rpc/* to the build
  // machine's localhost regardless of runtime BACKEND_*_URL values.
  // src/app/api/rpc/[module]/[...rpc]/route.ts handles these paths at
  // runtime instead.
};
module.exports = nextConfig;
