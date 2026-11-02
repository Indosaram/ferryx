import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { IconButton } from "./IconButton";

describe("IconButton sizes", () => {
  afterEach(cleanup);

  it("keeps the compact desktop sizes unchanged", () => {
    render(
      <>
        <IconButton label="Small">x</IconButton>
        <IconButton label="Compact" size="sm">x</IconButton>
      </>,
    );
    expect(screen.getByRole("button", { name: "Small" })).toHaveClass("size-7");
    expect(screen.getByRole("button", { name: "Compact" })).toHaveClass("size-6");
  });

  it("offers a 44px touch size for phone surfaces", () => {
    render(<IconButton label="Close" size="touch">x</IconButton>);
    expect(screen.getByRole("button", { name: "Close" })).toHaveClass("size-11");
  });
});
