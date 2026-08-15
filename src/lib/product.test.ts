import { describe, expect, it } from "vitest";

import { productName } from "./product";

describe("product metadata", () => {
  it("uses the public product name", () => {
    expect(productName).toBe("LLMWiki");
  });
});
