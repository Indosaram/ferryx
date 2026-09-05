import { it, expect } from "vitest";
import { ControlClient, opaqueTarget } from "./client";
it("uses the actual host-qualified endpoint and bearer mutation envelope", async () => {
  const target = {hostId:"remote",ownerId:"o",epoch:"18446744073709551615",backendSessionId:"same"};
  const client = new ControlClient("https://fixture.invalid", () => "fixture-token", async (input, init) => {
    expect(input).toBe(`https://fixture.invalid/api/v1/agents/${opaqueTarget(target)}/prompt`);
    expect(new Headers(init?.headers).get("authorization")).toBe("Bearer fixture-token");
    expect(JSON.parse(String(init?.body))).toEqual({requestId:"once",target,params:{text:"fixture"}});
    return new Response(JSON.stringify({ok:true,requestId:"once",data:{stage:"accepted"}}));
  });
  expect(await client.mutate("prompt","once",{text:"fixture"},target)).toEqual({stage:"accepted"});
});
it("rejects malformed inventory at the transport boundary", async () => {
  const client = new ControlClient("https://fixture.invalid",()=>"fixture",async()=>new Response(JSON.stringify({ok:true,data:{revision:1,items:[{target:{epoch:42}}],completeness:"complete",unavailableHosts:[]},requestId:""})));
  await expect(client.list()).rejects.toThrow();
});
