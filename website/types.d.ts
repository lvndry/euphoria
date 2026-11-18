/** Two whole numbers of an interval, larger first, such as [3, 2] for a fifth. */
type Ratio = [number, number];

/** A Camelot code: 1 to 12, and A for minor or B for major. */
interface CamelotKey {
  number: number;
  letter: string;
}
