import { PageBody } from "@/components/PageBody";
import { SettingsPanel } from "@/components/SettingsPanel";

export const metadata = { title: "Sidekick Settings" };

export default function SettingsPage() {
  return (
    <PageBody>
      <SettingsPanel />
    </PageBody>
  );
}
