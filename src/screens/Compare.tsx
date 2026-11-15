import { Empty, Screen } from "./Placeholder";

export default function Compare() {
  return (
    <Screen title="Compare" subtitle="Inspect copies and choose which to keep.">
      <Empty text="Select a duplicate group to compare its copies." />
    </Screen>
  );
}
