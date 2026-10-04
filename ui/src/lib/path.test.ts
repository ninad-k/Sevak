import { describe, expect, it } from "vitest";
import { parentPath } from "./path";

describe("parentPath", () => {
  it("goes up from a directory being typed into", () => {
    expect(parentPath("~/Documents/Work/re")).toBe("~/Documents/");
    expect(parentPath("~/Documents/Work/")).toBe("~/Documents/");
    expect(parentPath("~/Documents/")).toBe("~/");
  });

  it("works with backslashes and drive paths", () => {
    expect(parentPath("C:\\Users\\me\\Doc")).toBe("C:\\Users\\");
    expect(parentPath("C:\\Users\\me\\")).toBe("C:\\Users\\");
    expect(parentPath("C:\\Users\\")).toBe("C:\\");
    expect(parentPath("D:/data/logs/")).toBe("D:/data/");
  });

  it("works for absolute Unix paths", () => {
    expect(parentPath("/usr/local/bin/x")).toBe("/usr/local/");
    expect(parentPath("/usr/")).toBe("/");
  });

  it("has nowhere to go from a root", () => {
    expect(parentPath("~/")).toBeNull();
    expect(parentPath("/")).toBeNull();
    expect(parentPath("C:\\")).toBeNull();
    expect(parentPath("/etc")).toBeNull();
  });

  it("keeps a keyword in front of the path", () => {
    expect(parentPath("f ~/Documents/Work/")).toBe("f ~/Documents/");
    expect(parentPath("f ~/Documents/")).toBe("f ~/");
    expect(parentPath("open C:\\Users\\me\\x")).toBe("open C:\\Users\\");
  });

  it("is null for input that is not a path", () => {
    expect(parentPath("")).toBeNull();
    expect(parentPath("firefox")).toBeNull();
    expect(parentPath("g cats and dogs")).toBeNull();
    expect(parentPath("2/3")).toBeNull();
    expect(parentPath("~notapath/")).toBeNull();
  });

  it("does not offer a bare UNC server as a place to list", () => {
    expect(parentPath("\\\\server\\share\\dir\\")).toBe("\\\\server\\share\\");
    expect(parentPath("\\\\server\\share\\")).toBeNull();
  });

  it("keeps spaces inside directory names", () => {
    expect(parentPath("~/My Documents/Sub Folder/f")).toBe("~/My Documents/");
  });
});
