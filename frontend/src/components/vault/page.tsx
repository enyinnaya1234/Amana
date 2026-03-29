import { VaultDashboard } from "@/components/vault";

export default function VaultRoute() {
  return <VaultDashboard />;
}

import Avatar from '@/components/Avatar/Avatar';

export default function AvatarTestPage() {
  return (
    <div className="p-10 flex flex-col gap-6">
      
      <h1 className="text-xl font-semibold">Avatar Test</h1>

      {/* With Image */}
      <div className="flex gap-4 items-center">
        <Avatar src="/user.jpg" alt="Samuel Winner" />
        <Avatar src="/user.jpg" alt="Samuel Winner" size="lg" />
        <Avatar src="/user.jpg" alt="Samuel Winner" size="xl" />
      </div>

      {/* Fallback */}
      <div className="flex gap-4 items-center">
        <Avatar alt="Samuel Winner" fallback="SW" />
        <Avatar alt="Gift" />
        <Avatar alt="Amana Project" size="lg" />
      </div>

      {/* Verified */}
      <div className="flex gap-4 items-center">
        <Avatar src="/user.jpg" alt="Samuel" verified />
        <Avatar alt="Samuel Winner" fallback="SW" verified />
      </div>

      {/* Online */}
      <div className="flex gap-4 items-center">
        <Avatar src="/user.jpg" alt="Samuel" online />
        <Avatar alt="Samuel Winner" fallback="SW" online />
      </div>

      {/* Verified + Online */}
      <div className="flex gap-4 items-center">
        <Avatar src="/user.jpg" alt="Samuel" verified online />
        <Avatar alt="Samuel Winner" fallback="SW" verified online />
      </div>

    </div>
  );
}
