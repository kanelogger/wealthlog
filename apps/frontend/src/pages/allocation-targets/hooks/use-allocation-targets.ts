import { useQuery } from "@tanstack/react-query";
import { listAllocationTargets } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import type { AllocationTarget } from "@/lib/types";

const EMPTY_ALLOCATION_TARGETS: AllocationTarget[] = [];

export function useAllocationTargets() {
  const {
    data: targets,
    isLoading,
    isError,
  } = useQuery<AllocationTarget[], Error>({
    queryKey: [QueryKeys.ALLOCATION_TARGETS],
    queryFn: listAllocationTargets,
  });

  return { targets: targets ?? EMPTY_ALLOCATION_TARGETS, isLoading, isError };
}
