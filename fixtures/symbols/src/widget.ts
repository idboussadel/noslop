export class Widget {
  #liveSecret = 1;
  private deadSecret = 2;

  render(): number {
    return this.#liveSecret + this.helper();
  }

  private helper(): number {
    return 3;
  }

  private neverCalled(): number {
    return 4;
  }
}
