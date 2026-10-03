const REPOSITORY = "https://github.com/simnJS/hangar-ai";

/** Static by construction: no data, no session, nothing to fetch. */
export default function Home() {
  return (
    <main>
      <h1>Hangar Cloud</h1>
      <p>
        The hosted board behind <strong>Hangar.AI</strong>. It holds the kanban
        that a team and its coding agents share: the same columns, the same
        atomic claim, the same comments — but on a server instead of a file, so
        several people and the agents on their machines work off one board.
      </p>

      <h2>What it does</h2>
      <ul>
        <li>Teams and boards for people signed in with their account.</li>
        <li>
          Board tokens for machines: one token per board, revocable, hashed at
          rest.
        </li>
        <li>
          A REST API under <code>/api/v1</code> where an agent claims, comments
          on and moves tasks.
        </li>
        <li>
          Claiming a task is a single conditional write, so two agents can never
          hold the same one.
        </li>
      </ul>

      <hr />

      <footer>
        Part of the Hangar.AI project — <a href={REPOSITORY}>source on GitHub</a>.
      </footer>
    </main>
  );
}
