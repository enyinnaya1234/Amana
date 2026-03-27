/** Off-chain trade record (DB mirror). Status updates after on-chain events per product flow. */
export type TradeDbStatus =
  | "PENDING_SIGNATURE"
  | "CREATED"
  | "FUNDED"
  | "DELIVERED"
  | "COMPLETED"
  | "DISPUTED"
  | "CANCELLED";

export interface TradeRecord {
  id: number;
  tradeId: string;
  buyerAddress: string;
  sellerAddress: string;
  amountUsdc: string;
  status: TradeDbStatus;
}
