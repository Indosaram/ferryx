import { it, expect, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { HostSessionSelection } from "./HostSessionSelection";
import type { Agent } from "./client";
afterEach(cleanup);
it("selects exact host workspace and session locally", () => {
  const agents: Agent[] = ["a", "b"].map(hostId => ({target:{hostId,ownerId:"o",epoch:"1",backendSessionId:"same"},workspaceId:"w",label:hostId,state:"idle",revision:1,source:{kind:"lifecycle"}}));
  let selected: unknown;
  render(<HostSessionSelection agents={agents} selected={null} onSelect={t => {selected=t;}} />);
  fireEvent.click(screen.getByRole("button",{name:"b / w / b"}));
  expect(selected).toEqual(agents[1].target);
  expect(screen.getByTestId("host-select")).toBeTruthy();
  expect(screen.getByTestId("workspace-select")).toBeTruthy();
});
