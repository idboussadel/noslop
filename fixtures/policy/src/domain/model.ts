import { save } from "../infra/db"; // illegal: domain must not import infrastructure
import moment from "moment"; // banned-import

export function run(): number {
  fetch("/x"); // banned-effect: network inside the domain layer
  return save() + moment().unix();
}
