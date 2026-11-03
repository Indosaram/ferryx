import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { PhoneHomeHeader } from "./PhoneHomeHeader";

afterEach(cleanup);

describe("PhoneHomeHeader", () => {
  it("shows the destination title alone when no session context is open", () => {
    render(<PhoneHomeHeader title="Supervisor" />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Supervisor");
    expect(screen.queryByTestId("phone-home-context")).toBeNull();
  });

  it("shows the open session's context under the title, never inside the heading", () => {
    render(<PhoneHomeHeader title="Supervisor" contextTitle="ferryx / main" trailing={<button type="button">Account</button>} />);
    expect(screen.getByTestId("phone-home-context")).toHaveTextContent("ferryx / main");
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(/^Supervisor$/);
    expect(screen.getByRole("button", { name: "Account" })).toBeInTheDocument();
  });

  it("treats a blank context as none", () => {
    render(<PhoneHomeHeader title="Inbox" contextTitle="   " />);
    expect(screen.queryByTestId("phone-home-context")).toBeNull();
  });
});
